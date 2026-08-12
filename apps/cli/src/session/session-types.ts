import type {
  ApprovalReviewer,
  CapabilityPolicy,
  CheckpointStore,
  CommandRunnerRegistry,
  ExecutorBriefing,
  GDScriptDevelopmentService,
  GDScriptLanguageService,
  GitInspector,
  GodotDiagnostics,
  GodotInspector,
  GodotKnowledge,
  GodotProjectProbe,
  KnowledgeCoordinator,
  MilestoneManifest,
  ModelProvider,
  PlannerPort,
  ProjectionService,
  ReferenceMaterializerPort,
  ReferenceRegistry,
  RegisteredToolInfo,
  ResearchService,
  ResearchSourcePort,
  SandboxBackend,
  SandboxProfile,
  SelfReference,
  SiralosSecurity,
  TaskRuntime,
  TaskRuntimeSnapshotSources,
  Tool,
  UndoService,
  WorkspaceRevisionRegistry,
} from "@siralos/core";
import type { ProjectInstructionService } from "@siralos/core";

export interface SessionIO {
  ask(prompt: string): Promise<string | null>;
  write(text: string): void;
  clear(): void;
}

export interface SessionInfo {
  readonly workspaceRoot: string;
  readonly configPath: string;
  readonly policy: CapabilityPolicy;
  readonly profile: SandboxProfile;
  readonly provider: ModelProvider;
  readonly selfReference: SelfReference;
  readonly tools: readonly RegisteredToolInfo[];
  readonly security: SiralosSecurity;
  readonly git: GitInspector;
  readonly godot: GodotInspector;
  readonly godotProbe: GodotProjectProbe;
  readonly knowledge: GodotKnowledge;
  readonly diagnostics: GodotDiagnostics;
  readonly language: GDScriptLanguageService;
  readonly development: GDScriptDevelopmentService;
  readonly reviewer: ApprovalReviewer;
  readonly checkpoints: CheckpointStore;
  readonly undo: UndoService;
  readonly runners: CommandRunnerRegistry;
  readonly sandbox: SandboxBackend;
  readonly tasks: TaskRuntime;
  readonly taskSources: TaskRuntimeSnapshotSources;
  readonly projection: ProjectionService;
  readonly revisions: WorkspaceRevisionRegistry;
  readonly workspaceRead: Tool;
  readonly instructions: ProjectInstructionService;
  readonly projectKnowledge: KnowledgeCoordinator;
  readonly references: ReferenceRegistry;
  readonly referenceMaterializer: ReferenceMaterializerPort;
  readonly referenceConfigError: string | null;
  readonly research: ResearchService;
  readonly researchSources: readonly ResearchSourcePort[];
  readonly planner: PlannerPort;
  /** Executor briefing foundation: compiles the current task's bounded brief. */
  readonly briefing: ExecutorBriefing;
  /** The current milestone manifest and its evidence-backed acceptance status. */
  readonly milestoneManifest: MilestoneManifest;
}

export interface SessionControls {
  beginPrompt(): AbortController;
  endPrompt(): void;
  /** Abort the active prompt (command, approval, or response). */
  cancelActivePrompt(): boolean;
}
