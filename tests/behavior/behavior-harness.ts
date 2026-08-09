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
  type CheckpointStore,
  type GDScriptDevelopmentPreview,
  type GDScriptDevelopmentResult,
  type GDScriptDevelopmentStatus,
  type ModelProvider,
  type ModelRequest,
  type ProjectionService,
  type GDScriptDevelopmentService,
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
  createWorkspaceApplyTextChangesetTool,
  createWorkspaceFilePrimitives,
  createWorkspaceReadTool,
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
  /** Wrap the application provider in a recording provider for request assertions. */
  readonly recording?: boolean;
  /** Reviewer scenario for the quality stage (fake-change-reviewer). */
  readonly reviewerScenario?: "clean" | "high";
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
  readonly approvals: () => number;
  readonly status: () => GDScriptDevelopmentStatus;
  readonly development: GDScriptDevelopmentService;
  readonly workspaceRead: Tool;
  readonly parserControl: ReturnType<typeof createFakeDiagnosticsService>["control"];
  readonly languageControl: ReturnType<typeof createFakeLanguageService>["control"];
  /** Session revision registry (workspace-scoped, opaque handles). */
  readonly revisions: WorkspaceRevisionRegistry;
  /** Recorded provider requests (when `recording: true`). */
  readonly requests: () => readonly ModelRequest[];
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
  const reviewer: ApprovalReviewer = {
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
  const tools = createToolRegistry([
    workspaceReadTool,
    createWorkspaceApplyTextChangesetTool(development),
    createGodotDevelopmentStatusTool(development),
  ]);
  const { runtime, sources, now } = createBehaviorRuntime();
  const recording = options.recording === true ? createRecordingProvider() : null;
  const projection: ProjectionService | undefined =
    options.projection === true
      ? createProjectionService({
          policy: createDefaultPolicy("develop-offline"),
          profile: DEVELOP_OFFLINE_PROFILE,
          capacity: createRouteContextCapacity("develop-offline"),
          getTaskSnapshot: () => runtime.latestTask()?.snapshot() ?? null,
          getTaskRequest: () => runtime.latestTask()?.contract().request ?? null,
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
        })
      : undefined;
  const application = createSolarisApplication({
    provider: recording?.provider ?? createDeterministicFakeProvider(),
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
    approvals: () => approvals,
    status: () => development.status(),
    parserControl: parser.control,
    languageControl: language.control,
    revisions,
    development,
    workspaceRead: workspaceReadTool,
    requests: () => (recording === null ? [] : [...recording.requests]),
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
      flow = createDevelopmentTaskFlow({ runtime, sources, now });
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
