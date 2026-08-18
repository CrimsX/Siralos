import { describe, expect, it } from "vitest";
import { parseInput } from "./input/parse-input.js";
import {
  TASK_RUNTIME_VERSION,
  createCommandRunnerRegistry,
  createExecutorBriefing,
  createKnowledgeCoordinator,
  createProjectionService,
  createRouteContextCapacity,
  createWorkspaceRevisionRegistry,
  createDefaultPolicy,
  DEFAULT_EXECUTION_CONTRACT,
  createPreparedCommand,
  createSelfReference,
  createSiralosApplication,
  createSiralosSecurity,
  createAdHocTaskContract,
  createTaskRuntime,
  createTaskRuntimeSnapshot,
  createToolRegistry,
  DEVELOP_OFFLINE_PROFILE,
  GitError,
  INSPECT_PROFILE,
  S3M8_MILESTONE_MANIFEST,
  createEmptyGodotProjectProfile,
  type CheckpointStore,
  type SelfReference,
  type CommandRunner,
  type CommandToolPreparationResult,
  type DevelopmentQualityReport,
  type FileCheckpoint,
  type Tool,
  type ToolExecutionResult,
  type GitDiffResult,
  type GitInspector,
  type GitStatusResult,
  type GitWorkspaceStatus,
  type GodotCompatibilityAssessment,
  type GDScriptDevelopmentService,
  type GDScriptLanguageService,
  type GDScriptLSPSessionPreview,
  type GodotDiagnostics,
  type GodotDiscoveryResult,
  type GodotDoctorReport,
  type GodotDiagnosticPreview,
  type GodotInspector,
  type GodotKnowledge,
  type GodotProbePreview,
  type PreparedGDScriptCheck,
  type PreparedGDScriptSession,
  type GodotProjectProbe,
  type GodotProjectProfile,
  type GodotSelectedInstallation,
  type ModelEvent,
  type ModelProvider,
  type ModelRequest,
  type PreparedCommandTool,
  type PreparedGodotProbe,
  type SandboxBackend,
  type SandboxBackendStatus,
  type SiralosApplication,
  type SiralosSecurity,
  type UndoOutcome,
  type UndoService,
  type ApprovalReviewer,
  type ProjectInstructionService,
  type ReferenceMaterializerPort,
  type ReferenceRegistry,
  type ResearchService,
} from "@siralos/core";
import { createCliApplication } from "./bootstrap/create-application.js";
import { createInteractiveApprovalReviewer } from "./approval/approval-reviewer.js";
import { createInputQueue, type InputQueue } from "./input/input-queue.js";
import {
  createSessionControls,
  runInteractiveSession,
  type SessionIO,
  type SessionInfo,
} from "./interactive-session.js";

class ScriptedIO implements SessionIO {
  private readonly lines: readonly string[];
  private index = 0;
  private readonly chunks: string[] = [];

  constructor(lines: readonly string[]) {
    this.lines = lines;
  }

  ask(_prompt: string): Promise<string | null> {
    if (this.index >= this.lines.length) {
      return Promise.resolve(null);
    }
    const line = this.lines[this.index];
    this.index += 1;
    return Promise.resolve(line === undefined ? null : line);
  }

  write(text: string): void {
    this.chunks.push(text);
  }

  clear(): void {
    this.chunks.push("[clear]");
  }

  get text(): string {
    return this.chunks.join("");
  }
}

async function createComposedSession(lines: readonly string[]) {
  const io = new ScriptedIO(lines);
  const {
    application,
    workspaceRoot,
    tools,
    security,
    git,
    godot,
    godotProbe,
    knowledge,
    diagnostics,
    language,
    development,
    tasks,
    taskSources,
    projection,
    revisions,
    workspaceRead,
    instructions,
    projectKnowledge,
    checkpoints,
    undo,
    runners,
    sandbox,
    references,
    referenceMaterializer,
    referenceConfigError,
    research,
    researchSources,
    planner,
    briefing,
    milestoneManifest,
    configPath,
    policy,
    profile,
    provider,
    selfReference,
  } = await createCliApplication();
  const sessionInfo: SessionInfo = {
    workspaceRoot,
    configPath,
    policy,
    profile,
    provider,
    selfReference,
    tasks,
    taskSources,
    projection,
    revisions,
    workspaceRead,
    instructions,
    projectKnowledge,
    tools,
    security,
    git,
    godot,
    godotProbe,
    knowledge,
    diagnostics,
    language,
    development,
    reviewer: {
      review(): Promise<{ type: "deny"; reason: string }> {
        return Promise.resolve({ type: "deny", reason: "not configured" });
      },
    },
    checkpoints,
    undo,
    runners,
    sandbox,
    references,
    referenceMaterializer,
    referenceConfigError,
    research,
    researchSources,
    planner,
    briefing,
    milestoneManifest,
  };
  return { io, application, sessionInfo };
}

function createStubGit(): GitInspector {
  return {
    inspectRepository(): Promise<GitWorkspaceStatus> {
      return Promise.resolve({
        gitAvailable: false,
        gitVersion: null,
        repositoryState: "unavailable",
        repositoryRoot: null,
        message: "Git is not installed or not on PATH.",
      });
    },
    getStatus(): Promise<GitStatusResult> {
      return Promise.reject(new GitError("git_unavailable", "Git is not available."));
    },
    getDiff(): Promise<GitDiffResult> {
      return Promise.reject(new GitError("git_unavailable", "Git is not available."));
    },
  };
}

function createStubInstructionService(): ProjectInstructionService {
  const empty = {
    instructions: [],
    truncated: false,
    scannedDirectories: 0,
    scannedFiles: 0,
  };
  return {
    load: () => Promise.resolve(empty),
    refresh: () => Promise.resolve(empty),
    instructions: () => [],
    resolveForPath: () =>
      Promise.resolve({ instructions: [], conflicts: [], revision: "stub-instructions" }),
    resolveForPaths: () =>
      Promise.resolve({ instructions: [], conflicts: [], revision: "stub-instructions" }),
    revision: () => null,
  };
}

function createStubWorkspaceRead(): Tool {
  return {
    definition: {
      name: "workspace.read",
      description: "stub",
      inputSchema: { type: "object" },
    },
    execute(): Promise<ToolExecutionResult> {
      return Promise.resolve({ status: "failed", message: "stub workspace read" });
    },
  };
}

function createStubCheckpointStore(): CheckpointStore {
  return {
    prepare(): Promise<FileCheckpoint> {
      return Promise.reject(new Error("Not used in session tests."));
    },
    finalizeApplied(): Promise<FileCheckpoint> {
      return Promise.reject(new Error("Not used in session tests."));
    },
    markUndone(): Promise<FileCheckpoint> {
      return Promise.reject(new Error("Not used in session tests."));
    },
    markState(): Promise<FileCheckpoint> {
      return Promise.reject(new Error("Not used in session tests."));
    },
    get(): Promise<FileCheckpoint | null> {
      return Promise.resolve(null);
    },
    list(): Promise<readonly FileCheckpoint[]> {
      return Promise.resolve([]);
    },
    loadPreimage(): Promise<Uint8Array | null> {
      return Promise.resolve(null);
    },
  };
}

function createStubUndo(): UndoService {
  return {
    undo(): Promise<UndoOutcome> {
      return Promise.resolve({
        type: "failed",
        checkpointId: null,
        path: null,
        message: "No undo service available.",
      });
    },
  };
}

function buildSessionInfo(overrides: Partial<SessionInfo> = {}): SessionInfo {
  return {
    workspaceRoot: "/workspace",
    configPath: "/config.json",
    policy: createDefaultPolicy("develop-offline"),
    profile: DEVELOP_OFFLINE_PROFILE,
    provider: createStubProvider(),
    selfReference: createStubSelfReference(),
    tools: [],
    security: createFakeSecurity(),
    git: createStubGit(),
    godot: createStubGodotInspector(),
    godotProbe: createStubGodotProbe(),
    reviewer: {
      review(): Promise<{ type: "deny"; reason: string }> {
        return Promise.resolve({ type: "deny", reason: "stub reviewer denies" });
      },
    },
    knowledge: createStubKnowledge(),
    diagnostics: createStubDiagnostics(),
    language: createStubLanguageService(),
    checkpoints: createStubCheckpointStore(),
    undo: createStubUndo(),
    runners: createCommandRunnerRegistry([]),
    development: createStubDevelopmentService(),
    tasks: createTaskRuntime(),
    taskSources: {
      runtimeVersion: TASK_RUNTIME_VERSION,
      provider: { profileId: "stub", route: null },
      sandboxProfileId: "develop-offline",
      capabilityPolicyRevision: "stub-policy",
      workspaceIdentity: "/workspace",
      godotEngineFingerprint: null,
      workflow: null,
    },
    projection: createProjectionService({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: createRouteContextCapacity("develop-offline"),
    }),
    revisions: createWorkspaceRevisionRegistry({ workspaceFingerprint: "test-workspace" }),
    workspaceRead: createStubWorkspaceRead(),
    instructions: createStubInstructionService(),
    projectKnowledge: createKnowledgeCoordinator(),
    references: createStubReferenceRegistry(),
    referenceMaterializer: createStubReferenceMaterializer(),
    referenceConfigError: null,
    research: createStubResearchService(),
    researchSources: [],
    planner: {
      plan(): Promise<{ status: "failed"; message: string }> {
        return Promise.resolve({ status: "failed", message: "stub planner unavailable" });
      },
    },
    briefing: createExecutorBriefing({
      executionContract: DEFAULT_EXECUTION_CONTRACT,
      getTaskContract: () => null,
      getTaskSnapshot: () => null,
      getCurrentPlan: () => null,
    }),
    milestoneManifest: S3M8_MILESTONE_MANIFEST,
    sandbox: createStubBackend({
      backendId: "stub-backend",
      state: "available",
      platform: "linux",
      version: "0.0.0",
      capabilities: {
        filesystemReadRestriction: true,
        filesystemWriteRestriction: true,
        networkRestriction: true,
        processTreeRestriction: true,
        violationReporting: true,
      },
    }),
    ...overrides,
  };
}

function createStubReferenceRegistry(): ReferenceRegistry {
  return {
    list: () => [],
    get: () => undefined,
    revision: () => null,
    bindTask: () => ({ taskId: "stub", revisions: new Map(), boundAtMs: 0 }),
    boundRevision: () => null,
    refresh: () => Promise.resolve({ status: "failed", reason: "stub registry" }),
    declineReason: () => null,
    size: 0,
  };
}

function createStubReferenceMaterializer(): ReferenceMaterializerPort {
  return {
    materialize: () => Promise.resolve({ status: "unavailable", reason: "stub materializer" }),
    status: () => "unavailable",
  };
}

function createStubProvider(): ModelProvider {
  return {
    id: "stub-provider",
    toolCalling: true,
    stream() {
      return (async function* () {})();
    },
  };
}

function createStubSelfReference(): SelfReference {
  return createSelfReference({
    runtime: { version: "0.0.0", nodeMajor: 24, platform: "linux" },
    registeredTools: [],
    sandboxProfileId: "develop-offline",
    policy: createDefaultPolicy("develop-offline"),
  });
}

function createStubResearchService(): ResearchService {
  return {
    fetch: () => Promise.resolve({ status: "refused", reason: "stub research service" }),
    latestEvidence: () => [],
    activeRequestCount: () => 0,
    sourceKinds: () => [],
  };
}

function createStubDevelopmentService(): GDScriptDevelopmentService {
  return {
    support: () =>
      Promise.resolve({
        state: "unavailable",
        reason: "stub: development unavailable",
        platform: "linux",
      }),
    prepareStart: () => Promise.resolve({ status: "unavailable", message: "stub unavailable" }),
    start: () => Promise.resolve({ status: "unavailable", message: "stub unavailable" }),
    status: () => ({
      support: { available: false, reason: "stub: development unavailable", platform: "linux" },
      session: null,
    }),
    prepareChangeSet: () => Promise.resolve({ status: "unavailable", message: "stub unavailable" }),
    applyChangeSet: () =>
      Promise.resolve({ status: "unavailable", message: "stub unavailable", result: null }),
    languageQueryGate: () => ({ blocked: false, message: null }),
    validationStatus: () => null,
    qualityReport: () => null,
    runIndependentReview: () =>
      Promise.resolve({
        status: "failed",
        findings: [],
        message: "no eligible development change exists",
      }),
    completeFromProviderTurn: () => undefined,
    cancel: () => Promise.resolve({ status: "inactive", message: "no active workflow" }),
    close: () => Promise.resolve(),
  };
}

function createStubLanguageService(): GDScriptLanguageService {
  return {
    support(): Promise<{ state: "unavailable"; reason: string; platform: string }> {
      return Promise.resolve({
        state: "unavailable",
        reason: "stub: the language session is unavailable",
        platform: "linux",
      });
    },
    activeSession() {
      return null;
    },
    selectedEngine() {
      return Promise.resolve(null);
    },
    prepare(): Promise<{ status: "unavailable"; message: string }> {
      return Promise.resolve({
        status: "unavailable",
        message: "stub: the language session is unavailable",
      });
    },
    start(): Promise<{ status: "unavailable"; message: string }> {
      return Promise.resolve({
        status: "unavailable",
        message: "stub: the language session is unavailable",
      });
    },
    status() {
      return {
        state: "unavailable" as const,
        sessionId: null,
        engineVersion: null,
        projectName: null,
        startedAtMs: null,
        idleMs: null,
        capabilities: { diagnostics: false, hover: false, completion: false, definition: false },
        openDocumentCount: 0,
        diagnosticCount: 0,
        networkIsolation: "unavailable" as const,
      };
    },
    closeAll(): Promise<void> {
      return Promise.resolve();
    },
  };
}

function createStubKnowledge(): GodotKnowledge {
  return {
    support(): Promise<{ state: "unavailable"; reason: string; platform: string }> {
      return Promise.resolve({
        state: "unavailable",
        reason: "stub: API knowledge generation is unavailable",
        platform: "linux",
      });
    },
    refresh(): Promise<{ status: "unavailable"; message: string }> {
      return Promise.resolve({
        status: "unavailable",
        message: "stub: API knowledge generation is unavailable",
      });
    },
    search(): Promise<{ status: "unavailable"; message: string }> {
      return Promise.resolve({
        status: "unavailable",
        message: "stub: no knowledge base loaded",
      });
    },
    lookup(): Promise<{ status: "not_found"; message: string }> {
      return Promise.resolve({ status: "not_found", message: "stub: not found" });
    },
    status() {
      return {
        state: "unavailable" as const,
        reason: "stub: API knowledge generation is unavailable",
        platform: "linux",
        profile: null,
        cacheEnabled: false as const,
        schemaVersion: 1,
        manualChannel: null,
      };
    },
  };
}

function createStubDiagnostics(): GodotDiagnostics {
  return {
    support(): Promise<{ state: "unavailable"; reason: string; platform: string }> {
      return Promise.resolve({
        state: "unavailable",
        reason: "stub: GDScript diagnostics are unavailable",
        platform: "linux",
      });
    },
    prepare(): Promise<{ status: "unavailable"; message: string }> {
      return Promise.resolve({
        status: "unavailable",
        message: "stub: GDScript diagnostics are unavailable",
      });
    },
    execute(): Promise<{ status: "unavailable"; message: string }> {
      return Promise.resolve({
        status: "unavailable",
        message: "stub: GDScript diagnostics are unavailable",
      });
    },
    status() {
      return {
        state: "untrusted" as const,
        lastResult: null,
        lastManifestDigest: null,
        lastEngineVersion: null,
      };
    },
    disposeAll() {
      // stub: nothing to dispose.
    },
  };
}

function createStubGodotInspector(): GodotInspector {
  return {
    discover(): Promise<GodotDiscoveryResult> {
      return Promise.resolve({
        candidates: [],
        configuration: {
          activeInstallation: null,
          configuredCount: 0,
          discoverOnPath: false,
          overrides: [],
        },
        selected: null,
        rationale: ["No selectable Godot installation was discovered."],
        diagnostics: [],
      });
    },
    selected(): Promise<GodotSelectedInstallation | null> {
      return Promise.resolve(null);
    },
    projectProfile(): Promise<GodotProjectProfile> {
      return Promise.resolve(createEmptyGodotProjectProfile());
    },
    compatibility(): Promise<GodotCompatibilityAssessment> {
      return Promise.resolve({
        status: "no-project",
        severity: "info",
        reasons: ["No project."],
      });
    },
    doctor(): Promise<GodotDoctorReport> {
      return Promise.resolve({
        discovery: {
          candidates: [],
          configuration: {
            activeInstallation: null,
            configuredCount: 0,
            discoverOnPath: false,
            overrides: [],
          },
          selected: null,
          rationale: [],
          diagnostics: [],
        },
        project: createEmptyGodotProjectProfile(),
        compatibility: { status: "no-project", severity: "info", reasons: [] },
        cache: { schemaVersion: 1, cachedProfileCount: 0, enabled: true },
        sandbox: {
          state: "available",
          backendId: "stub-backend",
          filesystemReadRestriction: true,
          networkRestriction: true,
          filesystemWriteRestriction: true,
          processTreeRestriction: true,
        },
        degradedCapabilities: [],
        recoveryProbe: {
          state: "unavailable",
          reason: "stub: no identity-bound launch primitive",
          platform: "linux",
        },
        knowledge: {
          state: "unavailable",
          reason: "stub: no identity-bound launch primitive",
          platform: "linux",
        },
        diagnostics: {
          state: "unavailable",
          reason: "stub: no identity-bound launch primitive",
          platform: "linux",
        },
        probes: [],
      });
    },
  };
}

function createStubGodotProbe(): GodotProjectProbe {
  return {
    support(): Promise<{ state: "unavailable"; reason: string; platform: string }> {
      return Promise.resolve({
        state: "unavailable",
        reason: "stub: recovery-mode project probing is unavailable",
        platform: "linux",
      });
    },
    prepare(): Promise<{ status: "unavailable"; message: string }> {
      return Promise.resolve({
        status: "unavailable",
        message: "stub: recovery-mode project probing is unavailable",
      });
    },
    execute(): Promise<import("@siralos/core").GodotRecoveryProbeResult> {
      return Promise.resolve({
        status: "unavailable",
        engine: { installationId: "", version: "", executableFingerprint: "" },
        recoveryMode: true,
        mirror: {
          sourceFiles: 0,
          sourceBytes: 0,
          generatedGodotDirectory: false,
          generatedBytes: null,
          generatedFiles: null,
          importState: "import state unknown",
        },
        diagnostics: { errors: [], warnings: [], truncated: false },
        process: { exitCode: null, durationMs: 0, timedOut: false },
        workspaceIntegrity: { unchanged: true, bounded: false },
        cleanup: { completed: true },
        message: "stub: unavailable",
      });
    },
    status() {
      return {
        state: "untrusted",
        lastResult: null,
        lastManifestDigest: null,
        lastEngineVersion: null,
      };
    },
    disposeAll(): void {
      // no-op
    },
  };
}

function createStubBackend(status: SandboxBackendStatus): SandboxBackend {
  return {
    id: "stub-backend",
    inspect(): Promise<SandboxBackendStatus> {
      return Promise.resolve(status);
    },
    execute(): Promise<never> {
      throw new Error("Not used in session tests.");
    },
    close(): Promise<void> {
      return Promise.resolve();
    },
  };
}

function createFakeSecurity(status?: SandboxBackendStatus): SiralosSecurity {
  const resolvedStatus: SandboxBackendStatus = status ?? {
    backendId: "fake-backend",
    state: "available",
    platform: "linux",
    version: "0.0.0-fake",
    capabilities: {
      filesystemReadRestriction: true,
      filesystemWriteRestriction: true,
      networkRestriction: true,
      processTreeRestriction: true,
      violationReporting: true,
    },
  };
  return createSiralosSecurity({
    backend: createStubBackend(resolvedStatus),
    policy: createDefaultPolicy("inspect"),
    profile: INSPECT_PROFILE,
  });
}

describe("runInteractiveSession", { timeout: 30_000 }, () => {
  // These tests compose the full application per test; under a loaded
  // parallel vitest run the default 5s timeout intermittently cuts
  // them off (observed on three different tests across repeated full
  // gates, all passing in isolation). The larger bound acknowledges
  // the composition cost; it never weakens assertions.
  it("submits a prompt and renders the streamed response", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["hello", "/exit"]);
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("Siralos received: hello");
  });

  it("/plan runs plan-only planning and stops without executing", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "/plan Add health regeneration",
      "/exit",
    ]);
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    // The deterministic fake provider produced a validated structured plan.
    expect(io.text).toContain("Plan rev 1 — Full");
    expect(io.text).toContain(
      "Plan-only mode: no source was modified, no mutation approval was requested,",
    );
    expect(io.text).toContain("plan-only mode — execution not started");
    // No mutation approval was requested: the composed session's reviewer
    // denies everything, and the session still produced the plan.
  });

  it("preserves conversation history across prompts", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "first",
      "second",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Siralos received: first");
    expect(io.text).toContain("Siralos received: second");
  });

  it("ignores empty input", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["", "   ", "/exit"]);
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).not.toContain("Siralos received: ");
  });

  it("reports an invalid slash command", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["/bogus", "/exit"]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Unknown command: /bogus");
    expect(io.text).not.toContain("Siralos received:");
  });

  it("exits cleanly on end of input", async () => {
    const { io, application, sessionInfo } = await createComposedSession([]);
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
  });

  it("renders help and status", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "/help",
      "/status",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Available commands");
    expect(io.text).toContain("Provider: deterministic-fake");
    expect(io.text).toContain("Messages: 0");
    expect(io.text).toContain("Sandbox:");
  });

  it("starts an ad-hoc task and renders its host-owned status", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "/task Add a health component",
      "/task-status",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Task task-1 (contract revision 1)");
    expect(io.text).toContain("Phase: working");
    expect(io.text).toContain("Acceptance: 0/1 satisfied");
    expect(io.text).toContain("host-verified pending");
    expect(io.text).toContain("Completion: NOT allowed");
    expect(io.text).toContain("Progress: healthy");
  });

  it("allocates an unused ad-hoc task id when existing ids are sparse", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "/task Add a stamina component",
      "/exit",
    ]);
    sessionInfo.tasks.createTask({
      contract: createAdHocTaskContract("task-2", "Existing sparse task"),
      snapshot: createTaskRuntimeSnapshot(sessionInfo.taskSources),
      steps: [],
    });

    await runInteractiveSession(io, application, sessionInfo);

    expect(sessionInfo.tasks.getTask("task-3")).not.toBeNull();
    expect(io.text).toContain("Task task-3 (contract revision 1)");
  });

  it("renders an empty task status when no task exists", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["/task-status", "/exit"]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("No task is tracked yet.");
  });

  it("renders the compiled executor brief for the current task", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "/task Inspect the main scene file read-only",
      "/brief",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Executor brief (siralos-execution-contract rev 2)");
    expect(io.text).toContain("TASK\nInspect the main scene file read-only");
    expect(io.text).toContain("Milestone: S3M11 rev 1");
    expect(io.text).toContain("Fingerprint:");
    expect(io.text).toContain("S3M11.ROUTING.SURFACE");
  });

  it("renders no brief when no task is tracked yet", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["/brief", "/exit"]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("No task is tracked yet.");
  });

  it("renders the milestone manifest and its evidence-backed acceptance status", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "/task Add a health component",
      "/milestone",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Milestone S3M11 rev 1 — Unified Godot-Native Development Workflow");
    expect(io.text).toContain("S3M11.ROUTING.SURFACE [incomplete]");
    expect(io.text).toContain("Result: 0 pass, 0 fail");
    // A task about health has no scene/resource request, so the brief
    // carries no milestone — but the /milestone command still renders the
    // session's current manifest with the task's (empty) evidence.
  });

  it("clears the terminal without clearing conversation history", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "hello",
      "/clear",
      "/status",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("[clear]");
    expect(io.text).toContain("Messages: 2");
  });

  it("renders a provider failure and keeps the session alive", async () => {
    const failingProvider: ModelProvider = {
      id: "failing-stub",
      stream(): AsyncIterable<ModelEvent> {
        throw new Error("provider exploded");
      },
    };
    const application = createSiralosApplication({
      provider: failingProvider,
      tools: createToolRegistry([]),
    });
    const io = new ScriptedIO(["hello", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("provider exploded");
  });
});

describe("runInteractiveSession tool activity", () => {
  it("renders the current projection for /context", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "hello",
      "/context",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Context projection (mode generic)");
    expect(io.text).toContain("Stable:");
    expect(io.text).toContain("Contextual:");
    expect(io.text).toContain("Volatile:");
    expect(io.text).toContain("Estimated:");
    expect(io.text).toContain("Pressure:");
    expect(io.text).toContain("Tool ABI:");
  });

  it("lists the registered tools with classifications for /tools", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["/tools", "/exit"]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Available tools");
    expect(io.text).toContain("workspace.list");
    expect(io.text).toContain("workspace.read");
    expect(io.text).toContain("workspace.search");
    expect(io.text).toContain("(read-only, allowed)");
  });

  it("includes the workspace, sandbox, and tool counts in /status", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["/status", "/exit"]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Workspace:");
    // git.status and git.diff are always registered; the adapter gates
    // availability (unavailable backends never execute Git). The
    // development workflow adds workspace.apply_text_changeset and
    // godot.development_status to the registered tool count; the
    // self-reference adds self.read and self.search (Stage 3 milestone 6);
    // read-only scene/resource intelligence adds godot.inspect_scene,
    // godot.inspect_resource, and godot.dependencies (Stage 3 milestone 8).
    expect(io.text).toContain("Tools: 31");
    expect(io.text).toContain("Provider tools:");
    expect(io.text).toContain("Pending approval: no");
    expect(io.text).toContain("Process execution: denied");
    expect(io.text).toContain("Command runners: 2");
    expect(io.text).toContain("Last command exit: none");
    expect(io.text).toContain("Recovery probe: never run");
  });

  it("renders list-files tool activity and a final response", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["list files", "/exit"]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain('\u25CF workspace.list {"path":"."}');
    expect(io.text).toMatch(/^\s+\d+ entries/m);
    expect(io.text).toMatch(/Siralos inspected \d+ workspace entries\./);
  });

  it("renders read activity without exposing raw file contents", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "read README.md",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain('\u25CF workspace.read {"path":"README.md"}');
    expect(io.text).toContain("Siralos read README.md.");
    expect(io.text).not.toContain("interactive agent harness for programming");
  });

  it("renders search activity with the match count", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "search Siralos",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain('\u25CF workspace.search {"query":"Siralos","path":"."}');
    expect(io.text).toMatch(/Siralos found \d+ matching lines\./);
  });

  it("renders a tool failure and returns to the prompt", async () => {
    let turn = 0;
    const provider: ModelProvider = {
      id: "tool-failure-stub",
      async *stream(): AsyncIterable<ModelEvent> {
        turn += 1;
        if (turn === 1) {
          yield {
            type: "tool_call",
            callId: "call-1",
            toolName: "exploding.tool",
            input: { path: "." },
          };
          await Promise.resolve();
          yield { type: "completed" };
          return;
        }
        yield { type: "text_delta", text: "recovered" };
        await Promise.resolve();
        yield { type: "completed" };
      },
    };
    const tool: Tool = {
      definition: { name: "exploding.tool", description: "Fails", inputSchema: {} },
      execute(): Promise<ToolExecutionResult> {
        return Promise.resolve({
          status: "denied",
          message: "Path is outside the Siralos workspace.",
        });
      },
    };
    const application = createSiralosApplication({
      provider,
      tools: createToolRegistry([tool]),
    });
    const io = new ScriptedIO(["hello", "/status", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo({
      tools: [{ definition: tool.definition, capability: "workspace.write" }],
    });
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("\u2715 Path is outside the Siralos workspace.");
    expect(io.text).toContain("recovered");
    expect(io.text).toContain("Messages: 4");
  });
});

describe("runInteractiveSession sandbox diagnostics", () => {
  it("renders the capability rules for /permissions", async () => {
    const io = new ScriptedIO(["/permissions", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    const exitCode = await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("Profile: inspect");
    expect(io.text).toMatch(/workspace\.read\s+allow/);
    expect(io.text).toMatch(/workspace\.write\s+deny/);
    expect(io.text).toMatch(/process\.execute\s+deny/);
    expect(io.text).toMatch(/network\.outbound\s+deny/);
    expect(io.text).toContain(
      "Command execution requires one-time approval per exact command plan.",
    );
  });

  it("renders the sandbox status for /sandbox without secrets", async () => {
    const io = new ScriptedIO(["/sandbox", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    const exitCode = await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("Profile: inspect");
    expect(io.text).toContain("Backend: fake-backend");
    expect(io.text).toContain("State: available");
    expect(io.text).toContain("Network: denied");
    expect(io.text).toContain("Environment: minimal");
    expect(io.text).not.toContain("sk-");
  });

  it("renders setup-required guidance when the backend needs setup", async () => {
    const io = new ScriptedIO(["/sandbox", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo({
      security: createFakeSecurity({
        backendId: "fake-backend",
        state: "setup-required",
        platform: "windows",
        version: "0.0.0-fake",
        capabilities: {
          filesystemReadRestriction: false,
          filesystemWriteRestriction: false,
          networkRestriction: false,
          processTreeRestriction: false,
          violationReporting: false,
        },
        message: "Run the one-time elevated setup command.",
      }),
    });
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("State: setup-required");
    expect(io.text).toContain("Run the one-time elevated setup command.");
    expect(io.text).toContain("alpha");
  });
});

function createTestApplication() {
  return createSiralosApplication({
    provider: {
      id: "session-test-provider",
      async *stream(): AsyncIterable<ModelEvent> {
        yield { type: "text_delta", text: "ok" };
        await Promise.resolve();
        yield { type: "completed" };
      },
    },
    tools: createToolRegistry([]),
  });
}

describe("runInteractiveSession git and checkpoint commands", () => {
  it("renders git status for a non-repository workspace", async () => {
    const io = new ScriptedIO(["/git-status", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    const exitCode = await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("Git: unavailable");
    expect(io.text).toContain("Repository: unavailable");
  });

  it("renders /godot with a truthful no-installation summary", async () => {
    const io = new ScriptedIO(["/godot", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Selected installation: none");
    expect(io.text).toContain("No project code was executed.");
  });

  it("renders /godot-installations with candidates and rationale", async () => {
    const io = new ScriptedIO(["/godot-installations", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("No Godot installations were discovered.");
  });

  it("renders /godot-project for non-Godot workspaces", async () => {
    const io = new ScriptedIO(["/godot-project", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("No Godot project detected");
  });

  it("renders /godot-doctor without project execution", async () => {
    const io = new ScriptedIO(["/godot-doctor", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Siralos Godot doctor");
    expect(io.text).toContain("Recovery-mode project probe: unavailable");
    expect(io.text).toContain("No project code was executed.");
  });

  it("renders /godot-probe-status truthfully", async () => {
    const io = new ScriptedIO(["/godot-probe-status", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Project probe:");
    expect(io.text).toContain("Trust state: untrusted");
    expect(io.text).toContain("Last result: never run");
  });

  it("refuses /godot-probe without requesting approval when execution is unavailable", async () => {
    const io = new ScriptedIO(["/godot-probe", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("unavailable");
    expect(io.text).not.toContain("approval approved");
  });

  it("runs the full approved /godot-probe flow when execution is available", async () => {
    const io = new ScriptedIO(["/godot-probe", "/exit"]);
    const readyProbe: GodotProjectProbe = {
      support(): Promise<{ state: "available"; reason: null; platform: string }> {
        return Promise.resolve({ state: "available", reason: null, platform: "linux" });
      },
      prepare(): Promise<{
        status: "ready";
        probe: PreparedGodotProbe;
        preview: GodotProbePreview;
        digest: string;
      }> {
        return Promise.resolve({
          status: "ready",
          probe: {} as PreparedGodotProbe,
          preview: {
            projectName: "Fixture",
            engineVersion: "4.7.1.stable.official",
            installationId: "test-install",
            engineEdition: "standard",
            support: "verified",
            compatibility: "compatible",
            risks: {
              toolScripts: 1,
              enabledEditorPlugins: 0,
              gdextensions: 0,
              autoloads: 0,
              dotnetProjects: 0,
            },
            mirror: { estimatedFileCount: 3, estimatedBytes: 99 },
            isolation: {
              sourceWorkspace: "not-used-as-project",
              disposableMirror: true,
              recoveryMode: true,
              headless: true,
              network: "denied",
              environment: "minimal",
              stdin: "closed",
            },
            manifestDigest: "m".repeat(64),
          },
          digest: "d".repeat(64),
        });
      },
      execute(): Promise<import("@siralos/core").GodotRecoveryProbeResult> {
        return Promise.resolve({
          status: "unavailable",
          engine: {
            installationId: "test-install",
            version: "4.7.1.stable.official",
            executableFingerprint: "abc",
          },
          recoveryMode: true,
          mirror: {
            sourceFiles: 0,
            sourceBytes: 0,
            generatedGodotDirectory: false,
            generatedBytes: null,
            generatedFiles: null,
            importState: "import state unknown",
          },
          diagnostics: { errors: [], warnings: [], truncated: false },
          process: { exitCode: null, durationMs: 0, timedOut: false },
          workspaceIntegrity: { unchanged: true, bounded: false },
          cleanup: { completed: true },
          message: "unavailable",
        });
      },
      status() {
        return {
          state: "untrusted",
          lastResult: null,
          lastManifestDigest: null,
          lastEngineVersion: null,
        };
      },
      disposeAll(): void {
        // no-op
      },
    };
    const sessionInfo: SessionInfo = buildSessionInfo({
      godotProbe: readyProbe,
      reviewer: {
        review(): Promise<{ type: "approve_once" }> {
          return Promise.resolve({ type: "approve_once" });
        },
      },
    });
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Godot project probe requires approval.");
    expect(io.text).toContain("Static risk inventory:");
    expect(io.text).toContain("approval approved");
    expect(io.text).toContain("Recovery probe:");
  });

  it("keeps the session alive when Godot inspection fails", async () => {
    const failing: SessionInfo = buildSessionInfo({
      godot: {
        discover(): Promise<GodotDiscoveryResult> {
          return Promise.reject(new Error("probe exploded"));
        },
        selected(): Promise<GodotSelectedInstallation | null> {
          return Promise.reject(new Error("probe exploded"));
        },
        projectProfile(): Promise<GodotProjectProfile> {
          return Promise.reject(new Error("probe exploded"));
        },
        compatibility(): Promise<GodotCompatibilityAssessment> {
          return Promise.reject(new Error("probe exploded"));
        },
        doctor(): Promise<GodotDoctorReport> {
          return Promise.reject(new Error("probe exploded"));
        },
      },
    });
    const io = new ScriptedIO(["/godot", "/status", "/exit"]);
    await runInteractiveSession(io, createTestApplication(), failing);
    expect(io.text).toContain("probe exploded");
  });

  it("renders a diff failure without raw traces", async () => {
    const io = new ScriptedIO(["/diff", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Git is not available.");
    expect(io.text).not.toContain("at ");
  });

  it("rejects invalid diff scopes", async () => {
    const io = new ScriptedIO(["/diff bogus", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Usage: /diff");
  });

  it("lists checkpoints without preimage content", async () => {
    const io = new ScriptedIO(["/checkpoints", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo({
      checkpoints: {
        prepare() {
          return Promise.reject(new Error("not used"));
        },
        finalizeApplied() {
          return Promise.reject(new Error("not used"));
        },
        markUndone() {
          return Promise.reject(new Error("not used"));
        },
        markState() {
          return Promise.reject(new Error("not used"));
        },
        get() {
          return Promise.resolve(null);
        },
        list() {
          return Promise.resolve([
            {
              version: 1,
              id: "cp_01Jtest12345",
              workspaceFingerprint: "fingerprint",
              relativePath: "README.md",
              operation: "update",
              toolName: "workspace.edit_file",
              createdAt: new Date().toISOString(),
              state: "applied",
              before: { exists: true, sha256: "a", byteLength: 1 },
              after: { exists: true, sha256: "b", byteLength: 1 },
              preview: { addedLines: 1, removedLines: 1 },
            },
          ] as FileCheckpoint[]);
        },
        loadPreimage() {
          return Promise.resolve(null);
        },
      },
    });
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("cp_01Jtest12");
    expect(io.text).toContain("applied");
    expect(io.text).not.toContain("preimage");
  });

  it("renders undo failures", async () => {
    const io = new ScriptedIO(["/undo", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("No undo service available.");
  });

  it("includes git and checkpoint summaries in /status", async () => {
    const io = new ScriptedIO(["/status", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Git: unavailable");
    expect(io.text).toContain("Checkpoint: none");
    expect(io.text).toContain("Uncertain checkpoints: 0");
  });

  it("uses the atomic Godot status snapshot when the inspector provides one", async () => {
    let snapshotCalls = 0;
    let legacyCalls = 0;
    const base = createStubGodotInspector();
    const sessionInfo: SessionInfo = buildSessionInfo({
      godot: {
        ...base,
        statusSnapshot: () => {
          snapshotCalls += 1;
          const project = createEmptyGodotProjectProfile();
          return Promise.resolve({
            selected: null,
            project,
            compatibility: { status: "no-project", severity: "info", reasons: [] },
          });
        },
        selected: () => {
          legacyCalls += 1;
          return Promise.resolve(null);
        },
        projectProfile: () => {
          legacyCalls += 1;
          return Promise.resolve(createEmptyGodotProjectProfile());
        },
        compatibility: () => {
          legacyCalls += 1;
          return Promise.resolve({ status: "no-project", severity: "info", reasons: [] });
        },
      },
    });

    await runInteractiveSession(
      new ScriptedIO(["/status", "/exit"]),
      createTestApplication(),
      sessionInfo,
    );
    expect(snapshotCalls).toBe(1);
    expect(legacyCalls).toBe(0);
  });

  it("renders /commands with runners, sandbox, and limits", async () => {
    const io = new ScriptedIO(["/commands", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo({
      runners: createCommandRunnerRegistry([
        createStubRunner("npm-script"),
        createStubRunner("node-script"),
      ]),
    });
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("npm-script");
    expect(io.text).toContain("node-script");
    expect(io.text).toContain("available");
    expect(io.text).toContain("approval, read-only workspace, offline");
    expect(io.text).toContain("Sandbox: stub-backend (available)");
    expect(io.text).toContain("Active command: none");
    expect(io.text).toContain("Default timeout: 120 seconds");
    expect(io.text).toContain("stdout limit: 1 MiB");
    expect(io.text).toContain("Recent commands:");
    expect(io.text).not.toContain("C:\\Users");
  });

  it("checks command-runner availability concurrently", async () => {
    let firstStarted = false;
    let secondStarted = false;
    let resolveFirst!: (available: boolean) => void;
    let resolveSecond!: (available: boolean) => void;
    const first: CommandRunner = {
      ...createStubRunner("first"),
      isAvailable: () => {
        firstStarted = true;
        return new Promise((resolve) => {
          resolveFirst = resolve;
        });
      },
    };
    const second: CommandRunner = {
      ...createStubRunner("second"),
      isAvailable: () => {
        secondStarted = true;
        return new Promise((resolve) => {
          resolveSecond = resolve;
        });
      },
    };
    const pending = runInteractiveSession(
      new ScriptedIO(["/commands", "/exit"]),
      createTestApplication(),
      buildSessionInfo({ runners: createCommandRunnerRegistry([first, second]) }),
    );

    await new Promise<void>((resolve) => setImmediate(resolve));
    const startedTogether = firstStarted && secondStarted;
    resolveFirst(true);
    resolveSecond(true);
    await pending;
    expect(startedTogether).toBe(true);
  });

  it("reports /cancel when no command is active", async () => {
    const io = new ScriptedIO(["/cancel", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("No command is active.");
  });

  it("classifies process.run in /tools only when provider-accessible", async () => {
    const { tool } = createStubCommandTool();
    const { io, application, sessionInfo } = createCommandSession({
      lines: ["/tools", "/exit"],
      tool,
      turns: [[{ type: "completed" }]],
    });
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("process.run");
    expect(io.text).toContain("approval required");
  });

  it("runs an approved command, streams output, and returns to the prompt", async () => {
    const { tool } = createStubCommandTool({
      onOutputs: [
        { stream: "stdout", text: "line one\n" },
        { stream: "stderr", text: "warning\n" },
        { stream: "stdout", text: "unterminated tail" },
      ],
      result: {
        status: "success",
        output: {
          status: "completed",
          exitCode: 0,
          stdout: "line one\nunterminated tail",
          stderr: "warning\n",
          stdoutTruncated: false,
          stderrTruncated: false,
          durationMs: 1840,
          runnerId: "npm-script",
          commandDigest: "abc123",
        },
        summary: "Completed npm run check (exit 0).",
      },
    });
    const { io, application, sessionInfo } = createCommandSession({
      lines: ["run npm check", "y", "", "", "/exit"],
      tool,
      turns: [
        [
          {
            type: "tool_call",
            callId: "c1",
            toolName: "process.run",
            input: { runner: "npm-script", script: "check" },
          },
          { type: "completed" },
        ],
        [{ type: "completed" }],
      ],
    });
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("\u25CF npm run check");
    expect(io.text).toContain("  [stdout] line one");
    expect(io.text).toContain("  [stderr] warning");
    expect(io.text).toContain("  [stdout] unterminated tail");
    expect(io.text).toContain("\u2713 exit 0 in 1.8s");
    expect(io.text).toContain("> ");
  });

  it("renders denial, conflict, and timeout terminal states truthfully", async () => {
    const scenarios: {
      readonly result: ToolExecutionResult;
      readonly expected: string;
    }[] = [
      {
        result: { status: "timed_out", message: "timed out after 2.0 seconds" },
        expected: "timed out",
      },
      { result: { status: "conflict", message: "package.json changed" }, expected: "conflict" },
      { result: { status: "sandbox_denied", message: "write denied" }, expected: "failed" },
    ];
    for (const scenario of scenarios) {
      const { tool } = createStubCommandTool({ result: scenario.result });
      const { io, application, sessionInfo } = createCommandSession({
        lines: ["run npm check", "y", "", "", "/exit"],
        tool,
        turns: [
          [
            {
              type: "tool_call",
              callId: "c1",
              toolName: "process.run",
              input: { runner: "npm-script", script: "check" },
            },
            { type: "completed" },
          ],
          [{ type: "completed" }],
        ],
      });
      await runInteractiveSession(io, application, sessionInfo);
      expect(io.text).toContain(scenario.expected);
    }
  });

  it("cancels an active command via Ctrl+C and stays active", async () => {
    const { tool } = createStubCommandTool({
      result: { status: "cancelled", message: "The command was cancelled." },
    });
    const { application, sessionInfo } = createCommandSession({
      lines: ["run npm check", "y", ""],
      tool,
      turns: [
        [
          {
            type: "tool_call",
            callId: "c1",
            toolName: "process.run",
            input: { runner: "npm-script", script: "check" },
          },
          { type: "completed" },
        ],
      ],
    });
    const controls = createSessionControls();
    const io = new AbortTriggeringIO(["run npm check", "y", ""], () => {
      controls.cancelActivePrompt();
    });
    const sessionInfoWithControls: SessionInfo = sessionInfo;
    const exitCode = await runInteractiveSession(
      io,
      application,
      sessionInfoWithControls,
      controls,
    );
    expect(exitCode).toBe(0);
    expect(io.text).toContain("cancelled");
    expect(io.text).toContain("> ");
  });

  it("shows command state in /status after a completed command", async () => {
    const { tool } = createStubCommandTool();
    const { io, application, sessionInfo } = createCommandSession({
      lines: ["run npm check", "y", "", "", "/status", "/exit"],
      tool,
      turns: [
        [
          {
            type: "tool_call",
            callId: "c1",
            toolName: "process.run",
            input: { runner: "npm-script", script: "check" },
          },
          { type: "completed" },
        ],
        [{ type: "completed" }],
      ],
    });
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Active command: none");
    expect(io.text).toContain("Last command exit: 0");
    expect(io.text).toContain("Process execution: approval required");
  });

  it("does not let an approval timeout consume the next main-loop command", async () => {
    const { io, application, sessionInfo, inputQueue, text } = createTimedOutApprovalSession();
    const exitCode = await runInteractiveSession(
      io,
      application,
      sessionInfo,
      createSessionControls(),
      inputQueue,
    );
    expect(exitCode).toBe(0);
    expect(text()).toContain("denied");
    expect(text()).toContain("Siralos received: hello world");
  });
});

function createStubRunner(id: string): CommandRunner {
  return {
    definition: { id, description: `Stub ${id}` },
    prepare(): Promise<never> {
      return Promise.reject(new Error("Not used."));
    },
    toExecutionRequest(): Promise<never> {
      return Promise.reject(new Error("Not used."));
    },
    isAvailable(): Promise<boolean> {
      return Promise.resolve(true);
    },
  };
}

interface StubCommandToolOptions {
  readonly onOutputs?: readonly { readonly stream: "stdout" | "stderr"; readonly text: string }[];
  readonly result?: ToolExecutionResult;
  readonly emitDelayMs?: number;
}

function createStubCommandTool(options: StubCommandToolOptions = {}) {
  const preview = {
    runnerId: "npm-script",
    displayName: "npm run check",
    workingDirectory: ".",
    executableIdentity: "node v26.1.0 + npm 11.13.0",
    arguments: ["run", "check", "--"],
    timeoutMs: 120_000,
    stdoutLimitBytes: 1_048_576,
    stderrLimitBytes: 1_048_576,
    workspaceAccess: "read-only" as const,
    networkAccess: "denied" as const,
    environmentPolicy: "minimal" as const,
    stdinPolicy: "closed" as const,
  };
  const tool: PreparedCommandTool = {
    kind: "prepared_command",
    definition: {
      name: "process.run",
      description: "Run a validated Siralos development command.",
      inputSchema: { type: "object", properties: {}, required: [] },
    },
    capability: "process.execute",
    prepare(_input: unknown): Promise<CommandToolPreparationResult> {
      return Promise.resolve({
        status: "ready",
        command: createPreparedCommand(),
        preview,
        digest: "abcdef123456",
        commandId: "cmd-1",
      });
    },
    async executePrepared(_command, context): Promise<ToolExecutionResult> {
      for (const entry of options.onOutputs ?? []) {
        if (options.emitDelayMs !== undefined) {
          await new Promise<void>((resolve) => {
            setTimeout(resolve, options.emitDelayMs);
          });
        }
        context.onOutput?.({ type: entry.stream, text: entry.text });
      }
      return (
        options.result ?? {
          status: "success",
          output: {
            status: "completed",
            exitCode: 0,
            stdout: "",
            stderr: "",
            stdoutTruncated: false,
            stderrTruncated: false,
            durationMs: 0,
            runnerId: "npm-script",
            commandDigest: "abcdef123456",
          },
          summary: "Completed npm run check (exit 0).",
        }
      );
    },
  };
  return { tool };
}

function createCommandSession(options: {
  readonly lines: readonly string[];
  readonly tool: PreparedCommandTool;
  readonly turns: readonly (readonly ModelEvent[])[];
}) {
  const io = new ScriptedIO(options.lines);
  const inputQueue = createInputQueue(
    (prompt) => {
      io.write(prompt);
      return io.ask("");
    },
    (text) => io.write(text),
  );
  const provider = createScriptedProvider(options.turns);
  const application = createSiralosApplication({
    provider,
    tools: createToolRegistry([options.tool]),
    policy: createDefaultPolicy("develop-offline"),
    profile: DEVELOP_OFFLINE_PROFILE,
    reviewer: createInteractiveApprovalReviewer(inputQueue, 60_000),
  });
  const sessionInfo: SessionInfo = buildSessionInfo({
    runners: createCommandRunnerRegistry([]),
    tools: createToolRegistry([options.tool]).definitions(),
    security: createSiralosSecurity({
      backend: createStubBackend({
        backendId: "stub-backend",
        state: "available",
        platform: "linux",
        version: "0.0.0",
        capabilities: {
          filesystemReadRestriction: true,
          filesystemWriteRestriction: true,
          networkRestriction: true,
          processTreeRestriction: true,
          violationReporting: true,
        },
      }),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
    }),
  });
  return { io, application, sessionInfo, inputQueue };
}

function createTimedOutApprovalSession(): {
  io: SessionIO;
  application: SiralosApplication;
  sessionInfo: SessionInfo;
  inputQueue: InputQueue;
  text: () => string;
} {
  const chunks: string[] = [];
  const timedLines: Array<{ text: string; delayMs: number }> = [
    { text: "run npm check", delayMs: 0 },
    { text: "hello world", delayMs: 60 },
    { text: "/exit", delayMs: 0 },
  ];
  let lineIndex = 0;
  const io: SessionIO = {
    ask(_prompt: string): Promise<string | null> {
      const entry = timedLines[lineIndex];
      lineIndex += 1;
      if (entry === undefined) {
        return Promise.resolve(null);
      }
      return new Promise<string | null>((resolve) => {
        setTimeout(() => resolve(entry.text), entry.delayMs);
      });
    },
    write(text: string): void {
      chunks.push(text);
    },
    clear(): void {},
  };
  const inputQueue = createInputQueue(
    (prompt) => {
      io.write(prompt);
      return io.ask("");
    },
    (text) => io.write(text),
  );
  const { tool } = createStubCommandTool();
  let firstTurn = true;
  const provider: ModelProvider = {
    id: "echo-stub",
    stream(request: ModelRequest): AsyncIterable<ModelEvent> {
      const generator = {
        [Symbol.asyncIterator](): AsyncIterableIterator<ModelEvent> {
          const run = async function* (): AsyncIterableIterator<ModelEvent> {
            if (firstTurn) {
              firstTurn = false;
              yield {
                type: "tool_call",
                callId: "c1",
                toolName: "process.run",
                input: { runner: "npm-script", script: "check" },
              };
              yield { type: "completed" };
              return;
            }
            await Promise.resolve();
            const lastUser = [...request.messages]
              .reverse()
              .find((item) => item.type === "user_message");
            yield {
              type: "text_delta",
              text: `Siralos received: ${lastUser?.content ?? "?"}`,
            };
            yield { type: "completed" };
          };
          return run();
        },
      };
      return generator;
    },
  };
  const application = createSiralosApplication({
    provider,
    tools: createToolRegistry([tool]),
    policy: createDefaultPolicy("develop-offline"),
    profile: DEVELOP_OFFLINE_PROFILE,
    reviewer: createInteractiveApprovalReviewer(inputQueue, 20),
  });
  const sessionInfo: SessionInfo = buildSessionInfo();
  return { io, application, sessionInfo, inputQueue, text: () => chunks.join("") };
}

function createScriptedProvider(turns: readonly (readonly ModelEvent[])[]): ModelProvider {
  let index = 0;
  return {
    id: "scripted-stub",
    async *stream(): AsyncIterable<ModelEvent> {
      const events = turns[index] ?? [];
      index += 1;
      for (const event of events) {
        yield event;
        await Promise.resolve();
      }
    },
  };
}

class AbortTriggeringIO extends ScriptedIO {
  private readonly onAsk: () => void;
  private triggered = false;

  constructor(lines: readonly string[], onAsk: () => void) {
    super(lines);
    this.onAsk = onAsk;
  }

  override ask(prompt: string): Promise<string | null> {
    if (!this.triggered && prompt === "") {
      this.triggered = true;
      this.onAsk();
    }
    return super.ask(prompt);
  }
}

describe("runInteractiveSession Godot knowledge and diagnostics commands", () => {
  it("renders /godot-knowledge with a truthful unavailable status", async () => {
    const io = new ScriptedIO(["/godot-knowledge", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Godot API knowledge");
    expect(io.text).toContain("Knowledge status: unavailable");
    expect(io.text).toContain("Manual docs:         not locally synchronized");
  });

  it("renders /godot-api with an unavailable result when no knowledge base exists", async () => {
    const io = new ScriptedIO(["/godot-api Node owner", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("API search unavailable");
  });

  it("requires a query for /godot-api", async () => {
    const io = new ScriptedIO(["/godot-api", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Usage: /godot-api <query>");
  });

  it("refuses /godot-knowledge-refresh before any approval when generation is unavailable", async () => {
    const io = new ScriptedIO(["/godot-knowledge-refresh", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("unavailable");
    expect(io.text).not.toContain("approval approved");
    expect(io.text).not.toContain("Knowledge profile regenerated.");
  });

  it("refuses /gdscript-check before any approval when execution is unavailable", async () => {
    const io = new ScriptedIO(["/gdscript-check src/player/player.gd", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("unavailable");
    expect(io.text).not.toContain("approval approved");
  });

  it("requires a script path for /gdscript-check", async () => {
    const io = new ScriptedIO(["/gdscript-check", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Usage: /gdscript-check <relative-path>");
  });

  it("refuses /gdscript-diagnostics before any approval when execution is unavailable", async () => {
    const io = new ScriptedIO(["/gdscript-diagnostics", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("unavailable");
    expect(io.text).not.toContain("approval approved");
  });

  it("reports knowledge readiness in /status", async () => {
    const io = new ScriptedIO(["/status", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Knowledge: unavailable");
  });

  it("runs the full approved /gdscript-check flow when execution is available", async () => {
    const io = new ScriptedIO(["/gdscript-check src/player/player.gd", "/exit"]);
    const readyDiagnostics: GodotDiagnostics = {
      support(): Promise<{ state: "available"; reason: null; platform: string }> {
        return Promise.resolve({ state: "available", reason: null, platform: "linux" });
      },
      prepare(): Promise<{
        status: "ready";
        check: PreparedGDScriptCheck;
        preview: GodotDiagnosticPreview;
        digest: string;
      }> {
        return Promise.resolve({
          status: "ready",
          check: {} as PreparedGDScriptCheck,
          preview: {
            projectName: "Fixture",
            engineVersion: "4.7.1.stable.official",
            installationId: "test-install",
            engineEdition: "standard",
            support: "compatible-untested",
            compatibility: "compatible",
            scripts: { count: 1, paths: ["src/player/player.gd"], totalBytes: 64 },
            operation: "parse-only",
            isolation: {
              sourceWorkspace: "not-used-as-project",
              disposableMirror: true,
              checkOnly: true,
              headless: true,
              sceneExecution: "disabled",
              gameExecution: "disabled",
              network: "denied",
              environment: "minimal",
              stdin: "closed",
            },
            manifestDigest: "a".repeat(64),
          },
          digest: "b".repeat(64),
        });
      },
      execute(): Promise<{ status: "unavailable"; message: string }> {
        return Promise.resolve({
          status: "unavailable",
          message: "stub: execution gate refuses",
        });
      },
      status() {
        return {
          state: "untrusted" as const,
          lastResult: null,
          lastManifestDigest: null,
          lastEngineVersion: null,
        };
      },
      disposeAll() {
        // stub
      },
    };
    const reviewer: ApprovalReviewer = {
      review(): Promise<{ type: "approve_once" }> {
        return Promise.resolve({ type: "approve_once" });
      },
    };
    const sessionInfo: SessionInfo = buildSessionInfo({
      diagnostics: readyDiagnostics,
      reviewer,
    });
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("GDScript diagnostic probe");
    expect(io.text).toContain("Parse GDScript only (--check-only)");
    expect(io.text).toContain("approval approved");
    expect(io.text).toContain("GDScript diagnostics unavailable");
  });
});

describe("runInteractiveSession GDScript language commands", () => {
  it("refuses /gdscript-lsp before any approval when the session is unavailable", async () => {
    const io = new ScriptedIO(["/gdscript-lsp", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("unavailable");
    expect(io.text).not.toContain("approval approved");
  });

  it("stops /gdscript-lsp-stop without requiring approval", async () => {
    const io = new ScriptedIO(["/gdscript-lsp-stop", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("session stopped");
  });

  it("requires an active session for hover, completion, and definition commands", async () => {
    for (const command of [
      "/gdscript-hover src/player/player.gd 10 5",
      "/gdscript-complete src/player/player.gd 10 5",
      "/gdscript-definition src/player/player.gd 10 5",
    ]) {
      const io = new ScriptedIO([command, "/exit"]);
      const sessionInfo: SessionInfo = buildSessionInfo();
      await runInteractiveSession(io, createTestApplication(), sessionInfo);
      expect(io.text).toContain("No Godot language session is active");
    }
  });

  it("validates position arguments", async () => {
    const io = new ScriptedIO(["/gdscript-hover src/player/player.gd", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Usage: /gdscript-hover <relative-path> <line> <column>");
  });

  it("reports the language session in /status", async () => {
    const io = new ScriptedIO(["/status", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Godot LSP: inactive");
  });

  it("runs the full approved /gdscript-lsp flow when the session becomes available", async () => {
    const io = new ScriptedIO(["/gdscript-lsp", "/gdscript-lsp-stop", "/exit"]);
    const readyLanguage: GDScriptLanguageService = {
      support(): Promise<{ state: "available"; reason: null; platform: string }> {
        return Promise.resolve({ state: "available", reason: null, platform: "linux" });
      },
      activeSession() {
        return null;
      },
      selectedEngine() {
        return Promise.resolve(null);
      },
      prepare(): Promise<{
        status: "ready";
        session: PreparedGDScriptSession;
        preview: GDScriptLSPSessionPreview;
        digest: string;
      }> {
        return Promise.resolve({
          status: "ready",
          session: {} as PreparedGDScriptSession,
          preview: {
            projectName: "Fixture",
            engineVersion: "4.7.1.stable.official",
            installationId: "test-install",
            engineEdition: "standard",
            support: "compatible-untested",
            compatibility: "compatible",
            projectIntelligence: {
              gdscriptFiles: 2,
              toolScripts: 0,
              editorPlugins: 0,
              gdextensions: 0,
            },
            session: {
              sourceProject: "disposable mirror",
              godotMode: "headless recovery editor",
              lspNetwork: "loopback only",
              externalNetwork: "denied",
              sourceWrites: "denied",
              providerSecrets: "removed",
              lspMutations: "disabled",
            },
            capabilities: { diagnostics: true, hover: true, completion: true, definition: true },
            manifestDigest: "a".repeat(64),
          },
          digest: "b".repeat(64),
        });
      },
      start(): Promise<{ status: "unavailable"; message: string }> {
        return Promise.resolve({
          status: "unavailable",
          message: "stub: the execution gate refuses",
        });
      },
      status() {
        return {
          state: "unavailable" as const,
          sessionId: null,
          engineVersion: null,
          projectName: null,
          startedAtMs: null,
          idleMs: null,
          capabilities: { diagnostics: false, hover: false, completion: false, definition: false },
          openDocumentCount: 0,
          diagnosticCount: 0,
          networkIsolation: "unavailable" as const,
        };
      },
      closeAll(): Promise<void> {
        return Promise.resolve();
      },
    };
    const reviewer: ApprovalReviewer = {
      review(): Promise<{ type: "approve_once" }> {
        return Promise.resolve({ type: "approve_once" });
      },
    };
    const sessionInfo: SessionInfo = buildSessionInfo({
      language: readyLanguage,
      reviewer,
    });
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Godot GDScript language-server session requires approval");
    expect(io.text).toContain("LSP mutations        disabled");
    expect(io.text).toContain("approval approved");
    expect(io.text).toContain("GDScript language session stopped.");
  });
});

describe("runInteractiveSession development workflow commands", () => {
  it("rejects an empty /develop request with usage", async () => {
    const io = new ScriptedIO(["/develop", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Usage: /develop <request>");
  });

  it("refuses /develop before any approval when the workflow is unavailable", async () => {
    const io = new ScriptedIO(["/develop fix the parser", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("unavailable");
    expect(io.text).not.toContain("approval approved");
  });

  it("stops before the executor when exact full-plan approval is denied", async () => {
    let active = false;
    let cancelCalls = 0;
    let executorProviderCalls = 0;
    let approvalCalls = 0;
    const development: GDScriptDevelopmentService = {
      ...createStubDevelopmentService(),
      support: () => Promise.resolve({ state: "available", reason: null, platform: "linux" }),
      prepareStart: (request) =>
        Promise.resolve({
          status: "ready",
          workflowId: "workflow-plan-denial",
          preview: {
            request,
            projectName: "Test",
            projectFingerprint: "a".repeat(64),
            engineVersion: null,
            engineFingerprint: null,
            limits: {
              maxIterations: 4,
              maxRepairProposals: 3,
              maxFilesPerChangeSet: 8,
              maxReviewRounds: 3,
            },
            authorization: {
              sourceWrites: "each change set approved separately",
              languageSession: "read-only; recreated after approved edits under this approval",
              checkOnlyParsing: "covered",
              apiLookup: "covered",
              workspaceInspection: "covered",
              gitInspection: "covered",
              projectValidationCommands: "each command approved separately",
              independentReview: "read-only; fresh provider context",
              network: "denied",
              gameExecution: "disabled",
            },
          },
          digest: "b".repeat(64),
        }),
      start: (_workflowId, _context) => {
        active = true;
        return Promise.resolve({
          status: "ready",
          session: {
            id: "workflow-plan-denial",
            projectFingerprint: "a".repeat(64),
            engineFingerprint: null,
            request: "change the player",
            state: { kind: "active", phase: "investigating" },
            iteration: 0,
            repairProposalsUsed: 0,
            evidence: [],
            qualityReport: null,
          },
        });
      },
      status: () => ({
        support: { available: true, reason: null, platform: "linux" },
        session: active
          ? {
              id: "workflow-plan-denial",
              request: "change the player",
              state: { kind: "active" as const, phase: "investigating" as const },
              iteration: 0,
              maxIterations: 4,
              repairProposalsRemaining: 3,
              validation: null,
              appliedChangeSets: 0,
              errors: 0,
              warnings: 0,
              quality: {
                status: null,
                report: null,
                blockingFindings: 0,
                advisories: 0,
                reviewRoundsUsed: 0,
                maxReviewRounds: 3,
                repairRoundsUsed: 0,
                maxRepairRounds: 2,
              },
            }
          : null,
      }),
      cancel: () => {
        active = false;
        cancelCalls += 1;
        return Promise.resolve({ status: "cancelled", result: null });
      },
    };
    const reviewer: ApprovalReviewer = {
      review() {
        approvalCalls += 1;
        return Promise.resolve(
          approvalCalls === 1
            ? ({ type: "approve_once" } as const)
            : ({ type: "deny", reason: "plan needs revision" } as const),
        );
      },
    };
    const planner: SessionInfo["planner"] = {
      plan: () =>
        Promise.resolve({
          status: "ready",
          content: {
            objective: "Change the player safely",
            scope: { inScope: ["player"], outOfScope: [] },
            nonGoals: [],
            touchpoints: [],
            constraints: [],
            risks: [],
            steps: [
              {
                id: "step-1",
                title: "Prepare the player change",
                expectedTouchpoints: [],
                verification: ["user-approval"],
              },
            ],
            validation: { checks: ["repository check"] },
          },
        }),
    };
    const application = createSiralosApplication({
      provider: {
        id: "must-not-run",
        async *stream(): AsyncIterable<ModelEvent> {
          await Promise.resolve();
          executorProviderCalls += 1;
          yield { type: "text_delta", text: "unexpected executor call" };
          yield { type: "completed" };
        },
      },
      tools: createToolRegistry([]),
    });
    const io = new ScriptedIO(["/develop --plan change the player", "/exit"]);

    await runInteractiveSession(
      io,
      application,
      buildSessionInfo({ development, reviewer, planner }),
    );

    expect(io.text).toContain("plan denied: plan needs revision");
    expect(executorProviderCalls).toBe(0);
    expect(cancelCalls).toBe(1);
    expect(approvalCalls).toBe(2);
  });

  it("shows /development-status with no active workflow", async () => {
    const development: GDScriptDevelopmentService = {
      ...createStubDevelopmentService(),
      status: () => ({
        support: { available: true, reason: null, platform: "linux" },
        session: null,
      }),
    };
    const io = new ScriptedIO(["/development-status", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo({ development });
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("No development workflow is active");
  });

  it("shows /development-status with a bounded active workflow view", async () => {
    const development: GDScriptDevelopmentService = {
      ...createStubDevelopmentService(),
      status: () => ({
        support: { available: true, reason: null, platform: "linux" },
        session: {
          id: "wf-1",
          request: "fix the player parser",
          state: { kind: "active", phase: "reviewing" },
          iteration: 1,
          maxIterations: 4,
          repairProposalsRemaining: 3,
          validation: "clean",
          appliedChangeSets: 1,
          errors: 0,
          warnings: 1,
          quality: {
            status: null,
            report: null,
            blockingFindings: 0,
            advisories: 0,
            reviewRoundsUsed: 0,
            maxReviewRounds: 3,
            repairRoundsUsed: 0,
            maxRepairRounds: 2,
          },
        },
      }),
    };
    const io = new ScriptedIO(["/development-status", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo({ development });
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("State: reviewing");
    expect(io.text).toContain("Iteration: 1 / 4");
    expect(io.text).toContain("Validation: clean");
    expect(io.text).toContain("Repair proposals remaining: 3");
  });

  it("cancels an active development workflow and reports the truthful result", async () => {
    const development: GDScriptDevelopmentService = {
      ...createStubDevelopmentService(),
      status: () => ({
        support: { available: true, reason: null, platform: "linux" },
        session: {
          id: "wf-1",
          request: "fix the player parser",
          state: { kind: "active", phase: "investigating" },
          iteration: 0,
          maxIterations: 4,
          repairProposalsRemaining: 3,
          validation: null,
          appliedChangeSets: 0,
          errors: 0,
          warnings: 0,
          quality: {
            status: null,
            report: null,
            blockingFindings: 0,
            advisories: 0,
            reviewRoundsUsed: 0,
            maxReviewRounds: 3,
            repairRoundsUsed: 0,
            maxRepairRounds: 2,
          },
        },
      }),
      cancel: () =>
        Promise.resolve({
          status: "cancelled",
          result: {
            status: "cancelled",
            iterations: 0,
            changes: [],
            diagnostics: { errors: 0, warnings: 0 },
            validation: { parser: true, lsp: true, workspaceIntegrity: true },
            checkpointIds: [],
            quality: null,
          },
        }),
    };
    const io = new ScriptedIO(["/cancel", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo({ development });
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("cancelled");
    expect(io.text).toContain("Iterations: 0");
  });
});

describe("runInteractiveSession quality commands", () => {
  function qualityReport(status: DevelopmentQualityReport["status"]): DevelopmentQualityReport {
    return {
      developmentId: "wf-1",
      status,
      gates: [
        {
          id: "scope-verified",
          classification: "hard",
          status: status === "failed" ? "blocked" : "passed",
          summary: "scope summary",
          evidence: [{ kind: "scope", summary: "verified" }],
        },
        {
          id: "warnings",
          classification: "soft",
          status: status === "passed_with_advisories" ? "advisory" : "passed",
          summary: "warnings summary",
          evidence: [],
        },
        {
          id: "independent-review",
          classification: "hard",
          status: status === "blocking_findings" ? "blocked" : "passed",
          summary: "review summary",
          evidence: [],
        },
      ],
      review:
        status === "blocking_findings"
          ? {
              status: "completed",
              findings: [
                {
                  id: "abc123",
                  severity: "high",
                  category: "correctness",
                  title: "health can exceed max_health",
                  path: "scripts/player/player.gd",
                  line: 12,
                  evidence: "heal() adds without clamping",
                  impact: "health exceeds the maximum",
                  recommendation: "clamp the result",
                  confidence: "high",
                },
              ],
              blockingCount: 1,
              message: null,
            }
          : status === "passed_with_advisories"
            ? {
                status: "completed",
                findings: [
                  {
                    id: "def456",
                    severity: "medium",
                    category: "maintainability",
                    title: "helper used once",
                    path: null,
                    line: null,
                    evidence: "single call site",
                    impact: "minor",
                    recommendation: "inline it",
                    confidence: "high",
                  },
                ],
                blockingCount: 0,
                message: null,
              }
            : { status: "completed", findings: [], blockingCount: 0, message: null },
      repairRoundsUsed: 0,
      maxRepairRounds: 2,
      reviewRoundsUsed: 1,
      maxReviewRounds: 3,
      previousFindingIds: [],
      completedAtMs: 1,
    };
  }

  it("renders /quality with no report", async () => {
    const io = new ScriptedIO(["/quality", "/exit"]);
    await runInteractiveSession(io, createTestApplication(), buildSessionInfo());
    expect(io.text).toContain("No quality report exists yet");
  });

  it("renders /quality with a clean report", async () => {
    const development: GDScriptDevelopmentService = {
      ...createStubDevelopmentService(),
      qualityReport: () => qualityReport("passed"),
    };
    const io = new ScriptedIO(["/quality", "/exit"]);
    await runInteractiveSession(io, createTestApplication(), buildSessionInfo({ development }));
    expect(io.text).toContain("Development quality");
    expect(io.text).toContain("Gates:");
    expect(io.text).toContain("scope-verified (hard)");
    expect(io.text).toContain("Result: READY");
  });

  it("renders ready-with-advisories distinctly from clean", async () => {
    const development: GDScriptDevelopmentService = {
      ...createStubDevelopmentService(),
      qualityReport: () => qualityReport("passed_with_advisories"),
    };
    const io = new ScriptedIO(["/quality", "/exit"]);
    await runInteractiveSession(io, createTestApplication(), buildSessionInfo({ development }));
    expect(io.text).toContain("Result: READY WITH ADVISORIES");
    expect(io.text).toContain("Advisories:");
  });

  it("renders blocking findings by severity with evidence", async () => {
    const development: GDScriptDevelopmentService = {
      ...createStubDevelopmentService(),
      qualityReport: () => qualityReport("blocking_findings"),
    };
    const io = new ScriptedIO(["/quality", "/exit"]);
    await runInteractiveSession(io, createTestApplication(), buildSessionInfo({ development }));
    expect(io.text).toContain("Result: BLOCKING FINDINGS");
    expect(io.text).toContain("[high/high] health can exceed max_health");
    expect(io.text).toContain("heal() adds without clamping");
  });

  it("renders validation-incomplete status honestly", async () => {
    const development: GDScriptDevelopmentService = {
      ...createStubDevelopmentService(),
      qualityReport: () => qualityReport("validation_incomplete"),
    };
    const io = new ScriptedIO(["/quality", "/exit"]);
    await runInteractiveSession(io, createTestApplication(), buildSessionInfo({ development }));
    expect(io.text).toContain("Result: VALIDATION INCOMPLETE");
  });

  it("reports clearly when /review-change has no eligible change", async () => {
    const io = new ScriptedIO(["/review-change", "/exit"]);
    await runInteractiveSession(io, createTestApplication(), buildSessionInfo());
    expect(io.text).toContain("no eligible development change exists");
  });

  it("runs a fresh read-only review through /review-change without requesting approval", async () => {
    let reviewed = false;
    const development: GDScriptDevelopmentService = {
      ...createStubDevelopmentService(),
      runIndependentReview: () => {
        reviewed = true;
        return Promise.resolve({
          status: "completed",
          findings: [
            {
              id: "f1",
              severity: "low",
              category: "style",
              title: "minor style note",
              path: "scripts/player/player.gd",
              line: 3,
              evidence: "trailing whitespace",
              impact: "none",
              recommendation: "trim",
              confidence: "high",
            },
          ],
          message: null,
        });
      },
    };
    const io = new ScriptedIO(["/review-change", "/exit"]);
    await runInteractiveSession(io, createTestApplication(), buildSessionInfo({ development }));
    expect(reviewed).toBe(true);
    expect(io.text).toContain("Independent review: 1 finding(s)");
    expect(io.text).toContain("minor style note");
    expect(io.text).not.toContain("approval approved");
  });

  it("surfaces reviewer failure without crashing the CLI", async () => {
    const development: GDScriptDevelopmentService = {
      ...createStubDevelopmentService(),
      runIndependentReview: () =>
        Promise.resolve({
          status: "failed",
          findings: [],
          message: "the reviewer provider timed out",
        }),
    };
    const io = new ScriptedIO(["/review-change", "/exit"]);
    await runInteractiveSession(io, createTestApplication(), buildSessionInfo({ development }));
    expect(io.text).toContain("Independent review failed");
    expect(io.text).toContain("timed out");
  });

  it("returns to the prompt after a cancelled review", async () => {
    const controller = new AbortController();
    const development: GDScriptDevelopmentService = {
      ...createStubDevelopmentService(),
      runIndependentReview: (signal) => {
        signal?.addEventListener("abort", () => undefined);
        controller.abort();
        return Promise.resolve({ status: "cancelled", findings: [], message: "cancelled" });
      },
    };
    const io = new ScriptedIO(["/review-change", "/exit"]);
    await runInteractiveSession(io, createTestApplication(), buildSessionInfo({ development }));
    expect(io.text).toContain("Independent review cancelled");
  });

  it("shows the quality gate state in /development-status", async () => {
    const development: GDScriptDevelopmentService = {
      ...createStubDevelopmentService(),
      status: () => ({
        support: { available: true, reason: null, platform: "linux" },
        session: {
          id: "wf-1",
          request: "fix the player parser",
          state: { kind: "terminal", status: "completed_with_warnings" },
          iteration: 1,
          maxIterations: 4,
          repairProposalsRemaining: 3,
          validation: "clean",
          appliedChangeSets: 1,
          errors: 0,
          warnings: 1,
          quality: {
            status: "passed_with_advisories",
            report: qualityReport("passed_with_advisories"),
            blockingFindings: 0,
            advisories: 1,
            reviewRoundsUsed: 1,
            maxReviewRounds: 3,
            repairRoundsUsed: 0,
            maxRepairRounds: 2,
          },
        },
      }),
    };
    const io = new ScriptedIO(["/development-status", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo({ development });
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Quality: READY WITH ADVISORIES");
    expect(io.text).toContain("Blocking findings: 0");
  });

  it("shows a compact quality summary in /status when a workflow exists", async () => {
    const development: GDScriptDevelopmentService = {
      ...createStubDevelopmentService(),
      status: () => ({
        support: { available: true, reason: null, platform: "linux" },
        session: {
          id: "wf-1",
          request: "fix the player parser",
          state: { kind: "terminal", status: "completed" },
          iteration: 1,
          maxIterations: 4,
          repairProposalsRemaining: 3,
          validation: "clean",
          appliedChangeSets: 1,
          errors: 0,
          warnings: 0,
          quality: {
            status: "passed",
            report: qualityReport("passed"),
            blockingFindings: 0,
            advisories: 0,
            reviewRoundsUsed: 1,
            maxReviewRounds: 3,
            repairRoundsUsed: 0,
            maxRepairRounds: 2,
          },
        },
      }),
    };
    const io = new ScriptedIO(["/status", "/exit"]);
    await runInteractiveSession(io, createTestApplication(), buildSessionInfo({ development }));
    expect(io.text).toContain("Quality: READY");
  });

  it("parses the /quality and /review-change commands", () => {
    expect(parseInput("/quality")).toEqual({ type: "command", command: "quality", args: [] });
    expect(parseInput("/review-change")).toEqual({
      type: "command",
      command: "review-change",
      args: [],
    });
  });
});

describe("runInteractiveSession self-reference and doctor commands", () => {
  it("renders the installed-runtime identity for /siralos", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["/siralos", "/exit"]);
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("@siralos — installed Siralos runtime");
    expect(io.text).toContain("Version: 0.0.0");
    expect(io.text).toContain("Self-reference revision:");
    expect(io.text).toContain("Sections (self.read <section>):");
  });

  it("renders a bounded doctor report for /doctor", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["/doctor", "/exit"]);
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("Siralos Doctor");
    expect(io.text).toContain("runtime");
    expect(io.text).toContain(
      "Exit codes: 0 = no failures, 1 = one or more failures, 2 = invocation error.",
    );
  });

  it("filters /doctor to one area", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "/doctor providers",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Siralos Doctor");
    expect(io.text).toContain("providers");
    expect(io.text).not.toContain("runtime         PASS");
  });

  it("reports an unknown doctor area without ending the session", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "/doctor bogus",
      "/status",
      "/exit",
    ]);
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("Unknown doctor area: bogus");
    expect(io.text).toContain("Provider: deterministic-fake");
  });
});
