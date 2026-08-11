/**
 * Deterministic behavior-test harness (Stage 3 milestone 1).
 *
 * Behavior tests verify user-observable agent/runtime behavior at the
 * final observable boundary — the task runtime handle API and the full
 * application tool loop — rather than implementation internals. They are
 * network-free and deterministic: the deterministic fake provider, fake
 * language/parser/Git adapters, a real temp filesystem, a real checkpoint
 * store, and a fixed-clock task runtime.
 */
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import {
  DEVELOP_OFFLINE_PROFILE,
  TASK_RUNTIME_VERSION,
  canonicalizeJson,
  capabilityPolicyFingerprint,
  computeExecutorBriefFingerprint,
  createExecutorBriefing,
  createWorkspaceRevisionRegistry,
  sha256Hex,
  createDefaultPolicy,
  createDevelopmentTaskFlow,
  createProjectionService,
  createRouteContextCapacity,
  createSolarisApplication,
  createTaskRuntime,
  createTaskRuntimeSnapshot,
  createToolRegistry,
  resolveInstructionSet,
  type ApprovalReviewer,
  type CapabilitySnapshot,
  type CheckpointStore,
  type ExecutionContract,
  type ExecutorBrief,
  type GDScriptDevelopmentPreview,
  type GDScriptDevelopmentResult,
  type GDScriptDevelopmentStatus,
  type MilestoneManifest,
  type ModelProvider,
  type ModelRequest,
  type ProjectionService,
  type GDScriptDevelopmentService,
  type GodotSceneEvidenceView,
  type GodotSceneIntelligence,
  type ReferenceAccessPort,
  type RegisteredTool,
  type RegisteredToolInfo,
  type ReferenceEvidenceView,
  type ResearchService,
  type SolarisApplication,
  type TaskRuntime,
  type TaskRuntimeSnapshotSources,
  type TaskState,
  type Tool,
  type WorkspaceRevisionRegistry,
} from "@solaris/core";
import {
  createDeterministicFakeProvider,
  createFakeChangeReviewer,
  createFakeDiagnosticsService,
  createFakeGitInspector,
  createFakeLanguageService,
  createFilesystemCheckpointStore,
  createGDScriptDevelopmentService,
  createGodotDevelopmentStatusTool,
  createGodotDependenciesTool,
  createGodotReviewContextTool,
  createGodotInspectResourceTool,
  createGodotInspectSceneTool,
  createGodotSceneIntelligence,
  createReferenceTools,
  createWorkspaceApplyTextChangesetTool,
  createWorkspaceFilePrimitives,
  createWorkspaceReadTool,
  type ReferenceServices,
  type ReferenceTool,
} from "@solaris/adapters";

export const FIXTURE_PATH = "scripts/player/player.gd";
export const FIXTURE_CONTENT =
  "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tmove_and_slide()\n";

export function sha256Of(text: string): string {
  return createHash("sha256").update(text).digest("hex");
}

export function createBehaviorRuntime(): {
  readonly runtime: TaskRuntime;
  readonly sources: TaskRuntimeSnapshotSources;
  readonly now: () => number;
} {
  let tick = 1_000_000;
  const now = (): number => {
    tick += 10;
    return tick;
  };
  const policy = createDefaultPolicy("develop-offline");
  const sources: TaskRuntimeSnapshotSources = {
    runtimeVersion: TASK_RUNTIME_VERSION,
    provider: { profileId: "deterministic-fake", route: null },
    sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
    capabilityPolicyRevision: capabilityPolicyFingerprint(policy),
    workspaceIdentity: "<behavior-workspace>",
    godotEngineFingerprint: null,
    workflow: null,
  };
  return { runtime: createTaskRuntime({ now }), sources, now };
}

export function makeSnapshot(sources: TaskRuntimeSnapshotSources, now: () => number) {
  return createTaskRuntimeSnapshot(sources, now);
}

export interface TempWorkspace {
  readonly root: string;
  cleanup(): Promise<void>;
}

export async function createTempWorkspace(): Promise<TempWorkspace> {
  const root = await mkdtemp(join(tmpdir(), "solaris-behavior-"));
  return {
    root,
    cleanup: () => rm(root, { recursive: true, force: true }),
  };
}

/** Records every provider request for final-boundary assertions. */
export interface RecordingProvider {
  readonly provider: ModelProvider;
  readonly requests: ModelRequest[];
}

export function createRecordingProvider(
  inner: ModelProvider = createDeterministicFakeProvider(),
): RecordingProvider {
  const requests: ModelRequest[] = [];
  return {
    provider: {
      ...inner,
      stream(request: ModelRequest): AsyncIterable<import("@solaris/core").ModelEvent> {
        requests.push(request);
        return inner.stream(request);
      },
    },
    requests,
  };
}

export interface BehaviorLoopHarnessOptions {
  /** Wire the projection service (development mode) into the application. */
  readonly projection?: boolean;
  /**
   * Run the harness project in this pre-existing workspace root instead of
   * a fresh temp directory (caller owns its cleanup). Instruction files
   * placed there are visible to both discovery and the provider loop.
   */
  readonly workspaceRoot?: string;
  /**
   * Wire a project-instruction service into the projection (requires
   * `projection: true`).
   */
  readonly instructions?: import("@solaris/core").ProjectInstructionService;
  /** Wire a KnowledgeCoordinator into the projection (requires `projection: true`). */
  readonly knowledge?: import("@solaris/core").KnowledgeCoordinator;
  /**
   * Wire reference services (registry + access + tools) into the harness.
   * The read-only `reference.*` tools are registered and every successful
   * access call is recorded as a `ReferenceEvidenceView` (composition-root
   * style) so the `[Reference evidence]` projection section works with
   * `projection: true`.
   */
  readonly references?: ReferenceServices;
  /** Wire a research service into the `[Research evidence]` projection section (requires `projection: true`). */
  readonly research?: ResearchService;
  /**
   * Extra registered tools appended to the harness tool registry (e.g.
   * research tools registered under the deny-by-default research.fetch
   * capability, so the hidden-by-policy path is exercised through the real
   * ToolProjector and provider requests).
   */
  readonly extraTools?: readonly RegisteredTool[];
  /** Wrap the application provider in a recording provider for request assertions. */
  readonly recording?: boolean;
  /**
   * Wire read-only Godot scene/resource intelligence (Stage 3 milestone 8)
   * into the harness: registers `godot.inspect_scene` /
   * `godot.inspect_resource` / `godot.dependencies` against the harness
   * workspace and feeds `[Scene evidence]` projections.
   */
  readonly intelligence?: boolean;
  /**
   * Wire the executor briefing foundation into the harness: compiles the
   * bounded executor brief for the current task and feeds the
   * `[Executor brief]` projection section (requires `projection: true`).
   */
  readonly briefing?: {
    readonly executionContract: ExecutionContract;
    readonly milestone?: MilestoneManifest | null;
    readonly selectMilestone?: (request: string) => MilestoneManifest | null;
    readonly capabilitySnapshot?: CapabilitySnapshot | null;
    readonly workspaceScope?: import("@solaris/core").WorkspaceScope | null;
    readonly activeWorkingSet?: import("@solaris/core").ActiveWorkingSet | null;
    readonly documentationIndex?: readonly import("@solaris/core").DocumentationEntry[];
    readonly scopeSignals?: readonly import("@solaris/core").ScopeSignalRef[];
    readonly newFiles?: readonly import("@solaris/core").NewFileRationale[];
    readonly capabilityAreas?: readonly string[];
  };
  /** Replace the application provider entirely (scripted provider scenarios). */
  readonly providerOverride?: ModelProvider;
  /** Reviewer scenario for the quality stage (fake-change-reviewer). */
  readonly reviewerScenario?: "clean" | "high";
  /**
   * Override the interactive approval reviewer (workflow start, plan
   * approval, and every mutation approval flow through it). Defaults to
   * auto-approve.
   */
  readonly reviewerOverride?: ApprovalReviewer;
  /**
   * When false, the development service is created without the quality
   * stage so the workflow stays in the reviewing phase after an apply and
   * direct mutation sequences can be exercised end to end.
   */
  readonly qualityStage?: boolean;
}

export interface BehaviorLoopHarness {
  readonly workspace: TempWorkspace;
  readonly store: CheckpointStore;
  readonly application: SolarisApplication;
  readonly runtime: TaskRuntime;
  /** Projection service (when `projection: true`); null otherwise. */
  readonly projection: ProjectionService | null;
  readonly approvals: () => number;
  readonly status: () => GDScriptDevelopmentStatus;
  readonly development: GDScriptDevelopmentService;
  readonly workspaceRead: Tool;
  readonly parserControl: ReturnType<typeof createFakeDiagnosticsService>["control"];
  readonly languageControl: ReturnType<typeof createFakeLanguageService>["control"];
  /** Session revision registry (workspace-scoped, opaque handles). */
  readonly revisions: WorkspaceRevisionRegistry;
  /** Registered tool definitions (final boundary surface). */
  readonly tools: () => readonly RegisteredToolInfo[];
  /** Read-only Godot scene/resource intelligence (when `intelligence: true`). */
  readonly intelligence: GodotSceneIntelligence | null;
  /** Compiled executor brief for the current task (when `briefing` wired). */
  readonly briefing: () => ExecutorBrief | null;
  /** Fingerprint of the compiled executor brief; null when none. */
  readonly briefingFingerprint: () => string | null;
  /** Recorded scene/resource inspection observations feeding `[Scene evidence]`. */
  readonly sceneObservations: () => readonly GodotSceneEvidenceView[];
  /** Recorded provider requests (when `recording: true`). */
  readonly requests: () => readonly ModelRequest[];
  /** Reference tools registered when `references` was provided (read-only list/read/search). */
  readonly referenceTools: () => readonly ReferenceTool[];
  /** Recorded reference observations feeding the `[Reference evidence]` projection section. */
  readonly referenceObservations: () => readonly ReferenceEvidenceView[];
  /** Prepare + approve + start the workflow and create the task. */
  startWorkflow(request: string): Promise<GDScriptDevelopmentPreview>;
  /** Run the provider loop for a request (drains all tool rounds). */
  runPrompt(request: string): Promise<void>;
  /** Host finalization after a terminal workflow: fetches the result and evaluates the task gate. */
  finalizeTask(): Promise<TaskState | null>;
  /** Cancel an active workflow (host /cancel semantics) and finalize the task. */
  cancelWorkflow(): Promise<{
    readonly result: GDScriptDevelopmentResult | null;
    readonly task: TaskState | null;
  }>;
  cleanup(): Promise<void>;
}

const checkpointRoots: string[] = [];

async function createTempCheckpointStore(workspaceRoot: string): Promise<CheckpointStore> {
  const rootDirectory = await mkdtemp(join(tmpdir(), "solaris-behavior-cp-"));
  checkpointRoots.push(rootDirectory);
  return createFilesystemCheckpointStore({ workspaceRoot, rootDirectory });
}

export async function cleanupTempCheckpointDirs(): Promise<void> {
  for (const directory of checkpointRoots.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
}

export async function createBehaviorLoopHarness(
  options: BehaviorLoopHarnessOptions = {},
): Promise<BehaviorLoopHarness> {
  const workspace =
    options.workspaceRoot === undefined
      ? await createTempWorkspace()
      : {
          root: options.workspaceRoot,
          cleanup: (): Promise<void> => Promise.resolve(),
        };
  await writeFile(
    join(workspace.root, "project.godot"),
    '[application]\nconfig/name="behavior-fixture"\n',
    "utf8",
  );
  await mkdir(dirname(join(workspace.root, FIXTURE_PATH)), { recursive: true });
  await writeFile(join(workspace.root, FIXTURE_PATH), FIXTURE_CONTENT, "utf8");
  const store = await createTempCheckpointStore(workspace.root);
  const language = createFakeLanguageService();
  const parser = createFakeDiagnosticsService();
  let approvals = 0;
  const reviewer: ApprovalReviewer = options.reviewerOverride ?? {
    review(): Promise<{ type: "approve_once" }> {
      approvals += 1;
      return Promise.resolve({ type: "approve_once" });
    },
  };
  const gitFake = createFakeGitInspector();
  const fakeReviewer = createFakeChangeReviewer({
    scenario: options.reviewerScenario ?? "clean",
  });
  const revisions = createWorkspaceRevisionRegistry({
    workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: workspace.root })),
  });
  const development = createGDScriptDevelopmentService({
    workspaceRoot: workspace.root,
    platform: "linux",
    store,
    lock: { acquire: () => Promise.resolve(() => undefined) },
    language: language.service,
    diagnostics: parser.service,
    git: gitFake.git,
    revisions,
    canApplyIdentityBound: true,
    primitives: createWorkspaceFilePrimitives(workspace.root),
    // Stage 3 milestone 9: bounded impact context for the independent
    // reviewer, derived from the changed surfaces (read-only).
    reviewContextProvider: async (changedPaths) => {
      const current = intelligenceHolder.current;
      if (current === null) {
        return null;
      }
      const contract = runtime.latestTask()?.contract() ?? null;
      const result = await current.reviewContext({
        taskId: contract?.id ?? "develop-review",
        taskContractRevision: contract?.revision ?? 1,
        changedPaths,
      });
      return result.status === "ok" ? result.manifest : null;
    },
    ...(options.qualityStage === false
      ? {}
      : {
          qualityStage: {
            reviewer: fakeReviewer.reviewer,
            validation: {
              discovery: {
                discover: () => Promise.resolve({ packageScripts: null, unreadable: false }),
              },
              executor: {
                run: (step) =>
                  Promise.resolve({ step, status: "passed", exitCode: 0, summary: "ok" }),
              },
            },
          },
        }),
    idFactory: () => `wf-${Math.floor(Math.random() * 1_000_000)}`,
    settling: { hardTimeoutMs: 1000, pollIntervalMs: 1 },
  });
  const workspaceReadTool = createWorkspaceReadTool(workspace.root, { revisions });
  // Stage 3 milestone 8: read-only scene/resource intelligence (single
  // application-owned subsystem). Composition-root style wiring: the
  // service records bounded observations and the projection consumes them.
  const sceneObservations: GodotSceneEvidenceView[] = [];
  const intelligenceHolder: { current: GodotSceneIntelligence | null } = { current: null };
  const intelligence =
    options.intelligence === true
      ? createGodotSceneIntelligence({
          workspaceRoot: workspace.root,
          revisions,
          onInspection: (view) => {
            sceneObservations.push(view);
            while (sceneObservations.length > 64) {
              sceneObservations.shift();
            }
          },
        })
      : null;
  intelligenceHolder.current = intelligence;
  const sceneTools =
    intelligence === null
      ? []
      : [
          createGodotInspectSceneTool(intelligence),
          createGodotInspectResourceTool(intelligence),
          createGodotDependenciesTool(intelligence),
          createGodotReviewContextTool(intelligence),
        ];
  // Reference services (Stage 3 milestone 5): register the read-only
  // reference tools and record every successful access call as a
  // ReferenceEvidenceView — the composition root's job in production — so
  // the [Reference evidence] projection section observes real tool traffic.
  const referenceObservations: ReferenceEvidenceView[] = [];
  function recordObservation(view: ReferenceEvidenceView): void {
    referenceObservations.push(view);
    while (referenceObservations.length > 64) {
      referenceObservations.shift();
    }
  }
  const referenceAccess: ReferenceAccessPort | null =
    options.references === undefined
      ? null
      : {
          list: async (request) => {
            const result = await options.references!.access.list(request);
            if (result.status === "ok") {
              recordObservation({
                referenceId: result.referenceId,
                alias: result.alias,
                revision: result.revision,
                path: result.path,
                operation: "list",
                mode: null,
                sha256: null,
                evidenceId: null,
              });
            }
            return result;
          },
          read: async (request) => {
            const result = await options.references!.access.read(request);
            if (result.status === "ok") {
              recordObservation({
                referenceId: result.referenceId,
                alias: result.alias,
                revision: result.revision,
                path: result.path,
                operation: "read",
                mode: request.mode,
                sha256: result.sha256,
                evidenceId: null,
              });
            }
            return result;
          },
          search: async (request) => {
            const result = await options.references!.access.search(request);
            if (result.status === "ok") {
              recordObservation({
                referenceId: result.referenceId,
                alias: result.alias,
                revision: result.revision,
                path: request.path ?? "",
                operation: "search",
                mode: null,
                sha256: null,
                evidenceId: null,
              });
            }
            return result;
          },
        };
  const referenceTools =
    referenceAccess === null
      ? []
      : createReferenceTools({
          registry: options.references!.registry,
          access: referenceAccess,
        });
  const tools = createToolRegistry([
    workspaceReadTool,
    createWorkspaceApplyTextChangesetTool(development),
    createGodotDevelopmentStatusTool(development),
    ...sceneTools,
    ...referenceTools,
    ...(options.extraTools ?? []),
  ]);
  const { runtime, sources: baseSources, now } = createBehaviorRuntime();
  let sources = baseSources;
  // Executor briefing foundation: session-level briefing service. The
  // harness consumes the same core service the CLI composition root uses;
  // briefing semantics never live in the harness.
  const briefingService =
    options.briefing === undefined
      ? null
      : createExecutorBriefing({
          executionContract: options.briefing.executionContract,
          milestone: options.briefing.milestone ?? null,
          ...(options.briefing.selectMilestone === undefined
            ? {}
            : { selectMilestone: options.briefing.selectMilestone }),
          getTaskContract: () => runtime.latestTask()?.contract() ?? null,
          getTaskSnapshot: () => runtime.latestTask()?.snapshot() ?? null,
          getCurrentPlan: () => runtime.latestTask()?.currentPlan() ?? null,
          ...(options.instructions === undefined
            ? {}
            : {
                resolveInstructions: (focusPaths) => {
                  const safe = focusPaths.filter(isSafeRelativeFocusPath);
                  const set = resolveInstructionSet({
                    instructions: options.instructions!.instructions(),
                    paths: safe.length === 0 ? ["."] : safe,
                  });
                  return set.instructions.length === 0 ? null : set;
                },
              }),
          ...(options.briefing.capabilitySnapshot === undefined
            ? {}
            : {
                getCapabilitySnapshot: () => options.briefing!.capabilitySnapshot ?? null,
              }),
          ...(options.briefing.workspaceScope === undefined
            ? {}
            : { workspaceScope: options.briefing.workspaceScope }),
          ...(options.briefing.activeWorkingSet === undefined
            ? {}
            : { activeWorkingSet: options.briefing.activeWorkingSet }),
          ...(options.briefing.documentationIndex === undefined
            ? {}
            : { documentationIndex: options.briefing.documentationIndex }),
          ...(options.briefing.scopeSignals === undefined
            ? {}
            : { scopeSignals: options.briefing.scopeSignals }),
          ...(options.briefing.newFiles === undefined
            ? {}
            : { newFiles: options.briefing.newFiles }),
          ...(options.briefing.capabilityAreas === undefined
            ? {}
            : { capabilityAreas: options.briefing.capabilityAreas }),
        });
  if (briefingService !== null && options.briefing !== undefined) {
    sources = {
      ...baseSources,
      executionContract: {
        id: options.briefing.executionContract.id,
        revision: options.briefing.executionContract.revision,
      },
    };
  }
  const recording = options.recording === true ? createRecordingProvider() : null;
  const projection: ProjectionService | undefined =
    options.projection === true
      ? createProjectionService({
          policy: createDefaultPolicy("develop-offline"),
          profile: DEVELOP_OFFLINE_PROFILE,
          capacity: createRouteContextCapacity("develop-offline"),
          getTaskSnapshot: () => runtime.latestTask()?.snapshot() ?? null,
          getTaskRequest: () => runtime.latestTask()?.contract().request ?? null,
          getCurrentPlan: () => runtime.latestTask()?.currentPlan() ?? null,
          ...(options.instructions === undefined
            ? {}
            : {
                instructions: {
                  resolve: (focusPaths) => {
                    const safe = focusPaths.filter(isSafeRelativeFocusPath);
                    const set = resolveInstructionSet({
                      instructions: options.instructions!.instructions(),
                      paths: safe.length === 0 ? ["."] : safe,
                    });
                    return set.instructions.length === 0 ? null : set;
                  },
                },
              }),
          ...(options.knowledge === undefined
            ? {}
            : {
                knowledge: {
                  pinned: () => options.knowledge!.pinnedFacts(),
                  retrieve: (query) => options.knowledge!.retrieve(query),
                },
              }),
          ...(options.references === undefined
            ? {}
            : {
                references: {
                  list: () => options.references!.registry.list(),
                  latestEvidence: () => [...referenceObservations],
                },
              }),
          ...(options.research === undefined
            ? {}
            : {
                research: {
                  latestEvidence: () => options.research!.latestEvidence(),
                },
              }),
          ...(intelligence === null
            ? {}
            : {
                scenes: {
                  latestEvidence: () => [...sceneObservations],
                },
              }),
          ...(briefingService === null
            ? {}
            : { getExecutorBrief: () => briefingService.latestOrCompile() }),
        })
      : undefined;
  const application = createSolarisApplication({
    provider: options.providerOverride ?? recording?.provider ?? createDeterministicFakeProvider(),
    tools,
    policy: createDefaultPolicy("develop-offline"),
    profile: DEVELOP_OFFLINE_PROFILE,
    reviewer,
    ...(projection === undefined ? {} : { projection }),
    onProviderTurnCompleted: () => {
      development.completeFromProviderTurn();
    },
  });
  // One flow per workflow run, like the CLI's /develop handler; the
  // service's onEvent slot is reassigned for each run.
  let flow: ReturnType<typeof createDevelopmentTaskFlow> | null = null;
  return {
    workspace,
    store,
    application,
    runtime,
    projection: projection ?? null,
    approvals: () => approvals,
    status: () => development.status(),
    parserControl: parser.control,
    languageControl: language.control,
    revisions,
    tools: () => tools.definitions(),
    intelligence,
    briefing: () => (briefingService === null ? null : briefingService.latestOrCompile()),
    briefingFingerprint: () => (briefingService === null ? null : briefingService.fingerprint()),
    sceneObservations: () => [...sceneObservations],
    development,
    workspaceRead: workspaceReadTool,
    requests: () => (recording === null ? [] : [...recording.requests]),
    referenceTools: () => [...referenceTools],
    referenceObservations: () => [...referenceObservations],
    startWorkflow: async (request: string): Promise<GDScriptDevelopmentPreview> => {
      const prepared = await development.prepareStart(request);
      if (prepared.status !== "ready") {
        throw new Error(prepared.message);
      }
      const started = await development.start(prepared.workflowId, {
        approvedDigest: prepared.digest,
      });
      if (started.status !== "ready") {
        throw new Error(started.message);
      }
      flow = createDevelopmentTaskFlow({
        runtime,
        sources,
        now,
        ...(briefingService === null
          ? {}
          : {
              // Executor briefing foundation: the immutable task snapshot
              // records the manifest identity and initial brief fingerprint.
              snapshotExtras: ({ taskId, contract }) => {
                const brief = briefingService.compileForRequest(taskId, contract.request);
                return {
                  milestoneManifest: brief?.milestone ?? null,
                  executorBriefFingerprint:
                    brief === null ? null : computeExecutorBriefFingerprint(brief),
                };
              },
            }),
      });
      development.onEvent = (event) => {
        flow?.handleEvent(event);
      };
      flow.start(request, prepared.preview, prepared.digest);
      return prepared.preview;
    },
    runPrompt: async (request: string): Promise<void> => {
      for await (const _event of application.sendPrompt(request)) {
        // drain the bounded provider/tool loop
      }
    },
    finalizeTask: async (): Promise<TaskState | null> => {
      const status = development.status();
      const session = status.session;
      if (session === null || session.state.kind !== "terminal") {
        return null;
      }
      // The CLI fetches the final result through the same cancel call.
      const cancelled = await development.cancel();
      return flow === null
        ? null
        : flow.finish(status, cancelled.status === "cancelled" ? cancelled.result : null);
    },
    cancelWorkflow: async (): Promise<{
      readonly result: GDScriptDevelopmentResult | null;
      readonly task: TaskState | null;
    }> => {
      const cancelled = await development.cancel();
      const status = development.status();
      const result = cancelled.status === "cancelled" ? cancelled.result : null;
      return { result, task: flow === null ? null : flow.finish(status, result) };
    },
    cleanup: async (): Promise<void> => {
      await development.close();
      await workspace.cleanup();
      await cleanupTempCheckpointDirs();
    },
  };
}

export async function readWorkspaceFile(workspaceRoot: string, path: string): Promise<string> {
  return readFile(join(workspaceRoot, path), "utf8");
}

/** Focus paths must be safe workspace-relative paths (see ADR 0017). */
export function isSafeRelativeFocusPath(path: string): boolean {
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
