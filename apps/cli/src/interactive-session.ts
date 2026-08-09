import type {
  ApprovalReviewer,
  CheckpointStore,
  CommandRunnerRegistry,
  GDScriptLanguageService,
  GitInspector,
  GodotDiagnostics,
  GodotInspector,
  GodotKnowledge,
  GodotProjectProbe,
  GodotProjectProbeStatus,
  RegisteredToolInfo,
  SandboxBackend,
  SandboxBackendStatus,
  SolarisApplication,
  SolarisSecurity,
  UndoService,
} from "@solaris/core";
import { GitError } from "@solaris/core";
import { parseInput } from "./input/parse-input.js";
import type { InputQueue } from "./input/input-queue.js";
import type { StatusView } from "./output.js";
import {
  describeError,
  formatCancelReport,
  formatCheckpoints,
  formatCommandCompleted,
  formatCommandStarted,
  formatCommandTerminal,
  formatCommands,
  formatGitDiff,
  formatGitStatus,
  formatGodotDoctor,
  formatGodotInstallations,
  formatGodotCompletionResult,
  formatGodotDefinitionResult,
  formatGodotDiagnosticPreview,
  formatGodotDiagnosticsResult,
  formatGodotHoverResult,
  formatGodotKnowledgeStatus,
  formatGodotLSPSessionPreview,
  formatGodotLSPSessionStatus,
  formatGodotApiSearchResult,
  formatGodotProbePreview,
  formatGodotProbeResult,
  formatGodotProbeStatus,
  formatGodotProbeTerminal,
  formatGodotProject,
  formatGodotSummary,
  formatHelp,
  formatInvalidCommand,
  formatNoActiveCommand,
  formatPermissions,
  formatProviderFailure,
  formatSandbox,
  formatSandboxViolation,
  formatStatus,
  formatToolCancelled,
  formatToolCompleted,
  formatToolFailed,
  formatTools,
  formatToolStarted,
  formatUndoOutcome,
  sanitizeForDisplay,
  sanitizePathForDisplay,
  type CommandsView,
} from "./output.js";

export interface SessionIO {
  ask(prompt: string): Promise<string | null>;
  write(text: string): void;
  clear(): void;
}

export interface SessionInfo {
  readonly workspaceRoot: string;
  readonly tools: readonly RegisteredToolInfo[];
  readonly security: SolarisSecurity;
  readonly git: GitInspector;
  readonly godot: GodotInspector;
  readonly godotProbe: GodotProjectProbe;
  readonly knowledge: GodotKnowledge;
  readonly diagnostics: GodotDiagnostics;
  readonly language: GDScriptLanguageService;
  readonly reviewer: ApprovalReviewer;
  readonly checkpoints: CheckpointStore;
  readonly undo: UndoService;
  readonly runners: CommandRunnerRegistry;
  readonly sandbox: SandboxBackend;
}

const PROMPT = "> ";

export interface SessionControls {
  beginPrompt(): AbortController;
  endPrompt(): void;
  /** Abort the active prompt (command, approval, or response). */
  cancelActivePrompt(): boolean;
}

export function createSessionControls(): SessionControls {
  let controller: AbortController | undefined;
  return {
    beginPrompt(): AbortController {
      controller = new AbortController();
      return controller;
    },
    endPrompt(): void {
      controller = undefined;
    },
    cancelActivePrompt(): boolean {
      if (controller === undefined) {
        return false;
      }
      if (!controller.signal.aborted) {
        controller.abort();
      }
      return true;
    },
  };
}

export async function runInteractiveSession(
  io: SessionIO,
  application: SolarisApplication,
  sessionInfo: SessionInfo,
  controls: SessionControls = createSessionControls(),
  inputQueue?: InputQueue,
): Promise<number> {
  const inputBuffer: string[] = [];
  const nextInput = async (prompt: string): Promise<string | null> => {
    if (inputBuffer.length > 0) {
      return inputBuffer.shift() as string;
    }
    if (inputQueue === undefined) {
      return io.ask(prompt);
    }
    const outcome = await inputQueue.ask(prompt);
    return outcome.kind === "answer" ? outcome.value : null;
  };
  for (;;) {
    const input = await nextInput(PROMPT);
    if (input === null) {
      return 0;
    }
    const parsed = parseInput(input);
    switch (parsed.type) {
      case "prompt":
        await runPrompt(io, application, parsed.text, controls, inputBuffer, inputQueue);
        break;
      case "command":
        switch (parsed.command) {
          case "help":
            io.write(formatHelp());
            break;
          case "status":
            io.write(formatStatus(await buildStatusView(application, sessionInfo)));
            break;
          case "clear":
            io.clear();
            break;
          case "tools":
            io.write(formatTools(sessionInfo.tools, sessionInfo.security));
            break;
          case "sandbox":
            await runSandboxCheck(io, sessionInfo.security);
            break;
          case "permissions":
            io.write(
              formatPermissions(sessionInfo.security.policy, sessionInfo.security.profile.id),
            );
            break;
          case "git-status":
            await runGitStatusCommand(io, sessionInfo);
            break;
          case "diff":
            await runDiffCommand(io, sessionInfo, parsed.args);
            break;
          case "checkpoints":
            io.write(formatCheckpoints(await sessionInfo.checkpoints.list({ limit: 10 })));
            break;
          case "undo":
            await runUndoCommand(io, sessionInfo, parsed.args);
            break;
          case "commands":
            await runCommandsCommand(io, application, sessionInfo);
            break;
          case "godot":
            await runGodotCommand(io, sessionInfo);
            break;
          case "godot-installations":
            await runGodotInstallationsCommand(io, sessionInfo);
            break;
          case "godot-project":
            await runGodotProjectCommand(io, sessionInfo);
            break;
          case "godot-doctor":
            await runGodotDoctorCommand(io, sessionInfo);
            break;
          case "godot-probe":
            await runGodotProbeCommand(io, sessionInfo, controls);
            break;
          case "godot-probe-status":
            io.write(formatGodotProbeStatus(sessionInfo.godotProbe.status()));
            break;
          case "godot-knowledge":
            io.write(formatGodotKnowledgeStatus(sessionInfo.knowledge.status()));
            break;
          case "godot-knowledge-refresh":
            await runGodotKnowledgeRefreshCommand(io, sessionInfo, controls);
            break;
          case "godot-api":
            await runGodotApiCommand(io, sessionInfo, parsed.args);
            break;
          case "gdscript-check":
            await runGDScriptCheckCommand(io, sessionInfo, controls, parsed.args);
            break;
          case "gdscript-diagnostics":
            await runGDScriptDiagnosticsCommand(io, sessionInfo, controls);
            break;
          case "gdscript-lsp":
            await runGDScriptLSPCommand(io, sessionInfo, controls);
            break;
          case "gdscript-lsp-stop":
            await runGDScriptLSPStopCommand(io, sessionInfo);
            break;
          case "gdscript-hover":
            await runGDScriptPositionCommand(io, sessionInfo, "hover", parsed.args);
            break;
          case "gdscript-complete":
            await runGDScriptPositionCommand(io, sessionInfo, "complete", parsed.args);
            break;
          case "gdscript-definition":
            await runGDScriptPositionCommand(io, sessionInfo, "definition", parsed.args);
            break;
          case "cancel":
            io.write(formatNoActiveCommand());
            break;
          case "exit":
            return 0;
        }
        break;
      case "empty":
        break;
      case "invalid_command":
        io.write(formatInvalidCommand(parsed.input));
        break;
    }
  }
}

async function runCommandsCommand(
  io: SessionIO,
  application: SolarisApplication,
  sessionInfo: SessionInfo,
): Promise<void> {
  const availability: Record<string, boolean> = {};
  for (const runner of sessionInfo.runners.definitions) {
    const instance = sessionInfo.runners.get(runner.id);
    availability[runner.id] = (await instance?.isAvailable().catch(() => false)) ?? false;
  }
  const backendStatus: SandboxBackendStatus | null = await sessionInfo.sandbox
    .inspect()
    .catch(() => null);
  const decision = sessionInfo.security.evaluateCapability("process.execute");
  const view: CommandsView = {
    runners: sessionInfo.runners.definitions,
    runnerAvailability: availability,
    backendStatus,
    processDecision: decision.decision === "deny" ? "denied" : "approval required",
    activeCommandId: application.getStatus().activeCommandId,
    history: application.getCommandHistory(),
  };
  io.write(formatCommands(view));
}

async function buildStatusView(
  application: SolarisApplication,
  sessionInfo: SessionInfo,
): Promise<StatusView> {
  const inspection = await sessionInfo.git.inspectRepository().catch(() => null);
  const statusResult =
    inspection?.repositoryState === "repository"
      ? await sessionInfo.git.getStatus({}).catch(() => null)
      : null;
  const checkpoints = await sessionInfo.checkpoints.list().catch(() => []);
  const latestApplied = checkpoints.find((checkpoint) => checkpoint.state === "applied");
  const processDecision = sessionInfo.security.evaluateCapability("process.execute");
  const godotSelected = await sessionInfo.godot.selected().catch(() => null);
  const godotProject = await sessionInfo.godot.projectProfile().catch(() => null);
  const godotCompatibility = await sessionInfo.godot.compatibility().catch(() => null);
  return {
    status: application.getStatus(),
    workspaceRoot: sessionInfo.workspaceRoot,
    toolCount: sessionInfo.tools.length,
    providerToolCount: sessionInfo.tools.filter(
      (info) => sessionInfo.security.evaluateCapability(info.capability).decision !== "deny",
    ).length,
    profileId: sessionInfo.security.profile.id,
    gitRepositoryState: inspection?.repositoryState ?? "unavailable",
    gitBranch:
      inspection?.repositoryState === "repository" && statusResult !== null
        ? statusResult.branch.detached
          ? `(detached) ${statusResult.branch.oid ?? "unknown"}`
          : statusResult.branch.head
        : null,
    gitDirtyCount:
      statusResult === null
        ? 0
        : statusResult.changes.length +
          statusResult.conflicts.length +
          statusResult.untracked.length,
    latestCheckpoint: latestApplied === undefined ? null : shortenId(latestApplied.id),
    uncertainCheckpointCount: checkpoints.filter((checkpoint) => checkpoint.state === "uncertain")
      .length,
    processPermission:
      processDecision.decision === "deny"
        ? "denied"
        : processDecision.decision === "ask"
          ? "approval required"
          : "allowed",
    runnerCount: sessionInfo.runners.definitions.length,
    activeCommandId: application.getStatus().activeCommandId,
    lastCommandExitCode: application.getLastCommandExitCode(),
    commandProfile: "validation-offline",
    godotSelectedInstallation: godotSelected?.installationId ?? null,
    godotVersion: godotSelected?.version.raw ?? null,
    godotProjectDetected: godotProject?.detected ?? false,
    godotCompatibility: godotCompatibility?.status ?? null,
    godotWarningCount: godotProject?.warnings.length ?? 0,
    projectProbe: describeProjectProbe(sessionInfo.godotProbe.status()),
    knowledge: describeKnowledge(sessionInfo.knowledge.status()),
    languageSession: describeLanguageSession(sessionInfo.language.status()),
  };
}

function describeLanguageSession(status: {
  readonly state: string;
  readonly engineVersion: string | null;
  readonly networkIsolation: string;
}): string {
  if (status.state === "ready") {
    return `active (${status.engineVersion ?? "?"}, ${status.networkIsolation})`;
  }
  return "inactive";
}

function describeKnowledge(status: {
  readonly state: string;
  readonly reason: string | null;
}): string {
  if (status.state === "ready") {
    return "ready (exact engine API docs)";
  }
  return "unavailable";
}

function describeProjectProbe(status: {
  readonly state: string;
  readonly lastResult: GodotProjectProbeStatus["lastResult"];
}): string {
  if (status.lastResult === null) {
    return status.state === "probe-invalidated" ? "approval invalidated" : "never run";
  }
  const diagnostics = status.lastResult.diagnostics;
  const count = diagnostics.errors.length + diagnostics.warnings.length;
  const summary = count === 0 ? "no diagnostics" : `${count} diagnostic${count === 1 ? "" : "s"}`;
  return `${status.lastResult.status} with ${summary}`;
}

function shortenId(id: string): string {
  return id.length > 12 ? id.slice(0, 12) : id;
}

async function runGitStatusCommand(io: SessionIO, sessionInfo: SessionInfo): Promise<void> {
  try {
    const inspection = await sessionInfo.git.inspectRepository();
    const result =
      inspection.repositoryState === "repository" ? await sessionInfo.git.getStatus({}) : undefined;
    io.write(formatGitStatus(inspection, result));
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGitFailure(error)));
  }
}

async function runDiffCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  args: readonly string[],
): Promise<void> {
  const scope = args[0] ?? "working";
  if (args.length > 1 || !["working", "staged", "head"].includes(scope)) {
    io.write("Usage: /diff [working|staged|head]\n");
    return;
  }
  try {
    const result = await sessionInfo.git.getDiff({ scope: scope as "working" | "staged" | "head" });
    io.write(formatGitDiff(result));
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGitFailure(error)));
  }
}

async function runUndoCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  args: readonly string[],
): Promise<void> {
  if (args.length > 1) {
    io.write("Usage: /undo [checkpoint-id]\n");
    return;
  }
  io.write(`Undo checkpoint ${args[0] === undefined ? "(latest)" : args[0]}...\n`);
  const outcome = await sessionInfo.undo.undo(args[0]);
  io.write(formatUndoOutcome(outcome));
}

async function runGodotCommand(io: SessionIO, sessionInfo: SessionInfo): Promise<void> {
  try {
    const selected = await sessionInfo.godot.selected();
    const project = await sessionInfo.godot.projectProfile();
    const compatibility = await sessionInfo.godot.compatibility();
    io.write(formatGodotSummary(selected, compatibility, project.detected));
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGodotFailure(error)));
  }
}

async function runGodotInstallationsCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
): Promise<void> {
  try {
    const discovery = await sessionInfo.godot.discover();
    io.write(formatGodotInstallations(discovery));
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGodotFailure(error)));
  }
}

async function runGodotProjectCommand(io: SessionIO, sessionInfo: SessionInfo): Promise<void> {
  try {
    const project = await sessionInfo.godot.projectProfile();
    const compatibility = await sessionInfo.godot.compatibility();
    io.write(formatGodotProject(project, compatibility));
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGodotFailure(error)));
  }
}

async function runGodotDoctorCommand(io: SessionIO, sessionInfo: SessionInfo): Promise<void> {
  try {
    const report = await sessionInfo.godot.doctor();
    io.write(formatGodotDoctor(report));
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGodotFailure(error)));
  }
}

async function runGodotProbeCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
): Promise<void> {
  const controller = controls.beginPrompt();
  try {
    io.write("Checking recovery-probe capability\u2026\n");
    const support = await sessionInfo.godotProbe.support();
    if (support.state !== "available") {
      io.write(
        formatGodotProbeTerminal(
          "unavailable",
          support.reason ?? "Recovery-mode project probing is unavailable on this platform.",
        ),
      );
      return;
    }
    io.write("Preparing the Godot project probe\u2026\n");
    const prepared = await sessionInfo.godotProbe.prepare(controller.signal);
    if (prepared.status !== "ready") {
      io.write(formatGodotProbeTerminal(prepared.status, prepared.message));
      return;
    }
    io.write(formatGodotProbePreview(prepared.preview));
    const decision = await sessionInfo.reviewer.review(
      {
        id: "godot-probe",
        capability: "godot.probe_project",
        toolName: "godot.probe_project",
        summary: `recovery-mode project probe (${prepared.preview.risks.toolScripts} tool scripts, ${prepared.preview.risks.enabledEditorPlugins} plugins)`,
        preview: prepared.preview,
        digest: prepared.digest,
      },
      controller.signal,
    );
    if (decision.type !== "approve_once") {
      if (decision.type === "cancelled") {
        io.write("  \u2715 probe approval cancelled\n");
      } else {
        io.write(`  \u2715 probe denied: ${decision.reason ?? "not approved"}\n`);
      }
      return;
    }
    io.write("  approval approved\n");
    const result = await sessionInfo.godotProbe.execute(prepared.probe, {
      approvedDigest: prepared.digest,
      signal: controller.signal,
    });
    io.write(formatGodotProbeResult(result));
  } catch (error: unknown) {
    if (controller.signal.aborted) {
      io.write("  \u2715 probe cancelled\n");
      return;
    }
    io.write(formatProviderFailure(describeGodotFailure(error)));
  } finally {
    controls.endPrompt();
  }
}

async function runGDScriptLSPCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
): Promise<void> {
  const controller = controls.beginPrompt();
  try {
    const status = sessionInfo.language.status();
    if (status.state === "ready") {
      io.write(formatGodotLSPSessionStatus(status));
      return;
    }
    io.write("Checking GDScript language-session capability\u2026\n");
    const support = await sessionInfo.language.support();
    if (support.state !== "available") {
      io.write(
        formatGodotProbeTerminal(
          "unavailable",
          support.reason ?? "The Godot language session is unavailable on this platform.",
        ),
      );
      return;
    }
    io.write("Preparing the GDScript language session\u2026\n");
    const prepared = await sessionInfo.language.prepare(controller.signal);
    if (prepared.status !== "ready") {
      io.write(formatGodotProbeTerminal(prepared.status, prepared.message));
      return;
    }
    io.write(formatGodotLSPSessionPreview(prepared.preview));
    const decision = await sessionInfo.reviewer.review(
      {
        id: "gdscript-lsp",
        capability: "godot.lsp",
        toolName: "godot.lsp_session",
        summary: `Godot GDScript language session (${prepared.preview.projectIntelligence.gdscriptFiles} scripts)`,
        preview: prepared.preview,
        digest: prepared.digest,
      },
      controller.signal,
    );
    if (decision.type !== "approve_once") {
      if (decision.type === "cancelled") {
        io.write("  \u2715 session approval cancelled\n");
      } else {
        io.write(`  \u2715 session denied: ${decision.reason ?? "not approved"}\n`);
      }
      return;
    }
    io.write("  approval approved\n");
    const result = await sessionInfo.language.start(prepared.session, {
      approvedDigest: prepared.digest,
      signal: controller.signal,
    });
    if (result.status === "ready") {
      io.write(formatGodotLSPSessionStatus(result.session.getStatus()));
      return;
    }
    io.write(formatGodotProbeTerminal(result.status, result.message));
  } catch (error: unknown) {
    if (controller.signal.aborted) {
      io.write("  \u2715 session cancelled\n");
      return;
    }
    io.write(formatProviderFailure(describeGodotFailure(error)));
  } finally {
    controls.endPrompt();
  }
}

async function runGDScriptLSPStopCommand(io: SessionIO, sessionInfo: SessionInfo): Promise<void> {
  try {
    await sessionInfo.language.closeAll();
    io.write("GDScript language session stopped.\n");
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGodotFailure(error)));
  }
}

async function runGDScriptPositionCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  operation: "hover" | "complete" | "definition",
  args: readonly string[],
): Promise<void> {
  if (args.length < 3) {
    io.write(`Usage: /gdscript-${operation} <relative-path> <line> <column>\n`);
    return;
  }
  const path = args[0] ?? "";
  const line = Number.parseInt(args[1] ?? "", 10);
  const column = Number.parseInt(args[2] ?? "", 10);
  if (!Number.isInteger(line) || line < 1 || !Number.isInteger(column) || column < 1) {
    io.write("Line and column must be 1-based positive integers.\n");
    return;
  }
  const session = sessionInfo.language.activeSession();
  if (session === null) {
    io.write(
      "No Godot language session is active; start and approve one with /gdscript-lsp first.\n",
    );
    return;
  }
  try {
    if (operation === "hover") {
      const result = await session.hover({ path, line, column });
      io.write(formatGodotHoverResult(result));
    } else if (operation === "complete") {
      const result = await session.completion({ path, line, column });
      io.write(formatGodotCompletionResult(result));
    } else {
      const result = await session.definition({ path, line, column });
      io.write(formatGodotDefinitionResult(result));
    }
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGodotFailure(error)));
  }
}

function describeGodotFailure(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return describeError(error);
}

async function runGodotKnowledgeRefreshCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
): Promise<void> {
  const controller = controls.beginPrompt();
  try {
    io.write("Checking exact-engine API knowledge capability\u2026\n");
    const support = await sessionInfo.knowledge.support();
    if (support.state !== "available") {
      io.write(
        formatGodotProbeTerminal(
          "unavailable",
          support.reason ??
            "Exact-engine API knowledge generation is unavailable on this platform.",
        ),
      );
      return;
    }
    io.write("Regenerating the exact-engine API knowledge profile\u2026\n");
    const result = await sessionInfo.knowledge.refresh(controller.signal);
    if (result.status === "ready") {
      io.write("Knowledge profile regenerated.\n");
      io.write(formatGodotKnowledgeStatus(sessionInfo.knowledge.status()));
      return;
    }
    io.write(formatGodotProbeTerminal(result.status, result.message));
  } catch (error: unknown) {
    if (controller.signal.aborted) {
      io.write("  \u2715 knowledge refresh cancelled\n");
      return;
    }
    io.write(formatProviderFailure(describeGodotFailure(error)));
  } finally {
    controls.endPrompt();
  }
}

async function runGodotApiCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  args: readonly string[],
): Promise<void> {
  const query = args.join(" ").trim();
  if (query.length === 0) {
    io.write("Usage: /godot-api <query>\n");
    return;
  }
  const result = await sessionInfo.knowledge.search({ query });
  io.write(formatGodotApiSearchResult(result));
}

async function runGDScriptCheckCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
  args: readonly string[],
): Promise<void> {
  const scriptPath = args.join(" ").trim();
  if (scriptPath.length === 0) {
    io.write("Usage: /gdscript-check <relative-path>\n");
    return;
  }
  const controller = controls.beginPrompt();
  try {
    io.write("Checking GDScript diagnostic capability\u2026\n");
    const support = await sessionInfo.diagnostics.support();
    if (support.state !== "available") {
      io.write(
        formatGodotProbeTerminal(
          "unavailable",
          support.reason ?? "GDScript diagnostics are unavailable on this platform.",
        ),
      );
      return;
    }
    io.write("Preparing the GDScript check\u2026\n");
    const prepared = await sessionInfo.diagnostics.prepare(
      { paths: [scriptPath] },
      controller.signal,
    );
    if (prepared.status !== "ready") {
      io.write(formatGodotProbeTerminal(prepared.status, prepared.message));
      return;
    }
    io.write(formatGodotDiagnosticPreview(prepared.preview));
    const decision = await sessionInfo.reviewer.review(
      {
        id: "gdscript-check",
        capability: "godot.diagnose",
        toolName: "godot.check_script",
        summary: `GDScript check-only diagnostics (${scriptPath})`,
        preview: prepared.preview,
        digest: prepared.digest,
      },
      controller.signal,
    );
    if (decision.type !== "approve_once") {
      if (decision.type === "cancelled") {
        io.write("  \u2715 check approval cancelled\n");
      } else {
        io.write(`  \u2715 check denied: ${decision.reason ?? "not approved"}\n`);
      }
      return;
    }
    io.write("  approval approved\n");
    const result = await sessionInfo.diagnostics.execute(prepared.check, {
      approvedDigest: prepared.digest,
      signal: controller.signal,
    });
    io.write(formatGodotDiagnosticsResult(result));
  } catch (error: unknown) {
    if (controller.signal.aborted) {
      io.write("  \u2715 check cancelled\n");
      return;
    }
    io.write(formatProviderFailure(describeGodotFailure(error)));
  } finally {
    controls.endPrompt();
  }
}

async function runGDScriptDiagnosticsCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
): Promise<void> {
  const controller = controls.beginPrompt();
  try {
    io.write("Checking GDScript diagnostic capability\u2026\n");
    const support = await sessionInfo.diagnostics.support();
    if (support.state !== "available") {
      io.write(
        formatGodotProbeTerminal(
          "unavailable",
          support.reason ?? "GDScript diagnostics are unavailable on this platform.",
        ),
      );
      return;
    }
    io.write("Preparing the project-wide GDScript check\u2026\n");
    const prepared = await sessionInfo.diagnostics.prepare({}, controller.signal);
    if (prepared.status !== "ready") {
      io.write(formatGodotProbeTerminal(prepared.status, prepared.message));
      return;
    }
    io.write(formatGodotDiagnosticPreview(prepared.preview));
    const decision = await sessionInfo.reviewer.review(
      {
        id: "gdscript-diagnostics",
        capability: "godot.diagnose",
        toolName: "godot.check_project_scripts",
        summary: `GDScript check-only diagnostics (${prepared.preview.scripts.count} scripts)`,
        preview: prepared.preview,
        digest: prepared.digest,
      },
      controller.signal,
    );
    if (decision.type !== "approve_once") {
      if (decision.type === "cancelled") {
        io.write("  \u2715 diagnostics approval cancelled\n");
      } else {
        io.write(`  \u2715 diagnostics denied: ${decision.reason ?? "not approved"}\n`);
      }
      return;
    }
    io.write("  approval approved\n");
    const result = await sessionInfo.diagnostics.execute(prepared.check, {
      approvedDigest: prepared.digest,
      signal: controller.signal,
    });
    io.write(formatGodotDiagnosticsResult(result));
  } catch (error: unknown) {
    if (controller.signal.aborted) {
      io.write("  \u2715 diagnostics cancelled\n");
      return;
    }
    io.write(formatProviderFailure(describeGodotFailure(error)));
  } finally {
    controls.endPrompt();
  }
}

function describeGitFailure(error: unknown): string {
  if (error instanceof GitError) {
    return error.message;
  }
  return describeError(error);
}

async function runSandboxCheck(io: SessionIO, security: SolarisSecurity): Promise<void> {
  for await (const event of security.checkSandbox()) {
    switch (event.type) {
      case "sandbox_check_started":
        io.write("Checking sandbox\u2026\n");
        break;
      case "sandbox_check_completed":
        io.write(formatSandbox(event.status, security.profile));
        break;
      case "sandbox_violation":
        io.write(formatSandboxViolation(event.category, sanitizeForDisplay(event.summary)));
        break;
    }
  }
}

async function runPrompt(
  io: SessionIO,
  application: SolarisApplication,
  text: string,
  controls: SessionControls,
  inputBuffer: string[],
  inputQueue?: InputQueue,
): Promise<void> {
  const controller = controls.beginPrompt();
  let busy: Promise<void> | undefined;
  let busyStarted = false;
  let promptFinished = false;
  let commandRenderer: CommandOutputRenderer | undefined;
  try {
    for await (const event of application.sendPrompt(text, controller.signal)) {
      switch (event.type) {
        case "response_started":
          io.write("\n");
          break;
        case "text_delta":
          io.write(event.text);
          break;
        case "response_completed":
          io.write("\n");
          break;
        case "response_cancelled":
          io.write("\n[response cancelled]\n");
          break;
        case "response_failed":
          io.write(`\n${formatProviderFailure(event.message)}`);
          break;
        case "tool_started":
          if (!isCommandTool(event.toolName)) {
            io.write(formatToolStarted(event.toolName, sanitizeForDisplay(event.displayInput)));
          }
          break;
        case "tool_completed":
          if (!isCommandTool(event.toolName)) {
            io.write(formatToolCompleted(sanitizeForDisplay(event.summary)));
          }
          break;
        case "tool_failed":
          if (!isCommandTool(event.toolName)) {
            io.write(formatToolFailed(sanitizeForDisplay(event.message)));
          }
          break;
        case "tool_cancelled":
          if (!isCommandTool(event.toolName)) {
            io.write(formatToolCancelled());
          }
          break;
        case "tool_awaiting_approval":
          if (!isCommandTool(event.toolName)) {
            io.write(`  \u23F3 ${event.toolName} awaiting approval\n`);
          }
          break;
        case "approval_requested":
          if (event.capability !== "process.execute") {
            io.write(`\nApproval required for ${event.toolName} (${event.summary})\n`);
          }
          break;
        case "approval_resolved":
          io.write(
            `  approval ${event.decision === "approved" ? "approved" : event.decision === "denied" ? "denied" : "cancelled"}\n`,
          );
          break;
        case "checkpoint_applied":
          io.write(
            `\u25CF Checkpoint ${event.checkpointId} recorded (${sanitizePathForDisplay(event.path)})\n`,
          );
          break;
        case "command_prepared":
          break;
        case "command_started":
          if (!busyStarted) {
            busyStarted = true;
            busy = runBusyInput(io, controller, inputBuffer, () => promptFinished, inputQueue);
          }
          io.write(formatCommandStarted(event.displayName, event.digestPrefix));
          commandRenderer = createCommandOutputRenderer((text) => io.write(text));
          break;
        case "command_stdout":
          commandRenderer?.stdout(event.text);
          break;
        case "command_stderr":
          commandRenderer?.stderr(event.text);
          break;
        case "command_completed":
          commandRenderer?.flush();
          commandRenderer = undefined;
          io.write(formatCommandCompleted(event.exitCode, event.durationMs));
          break;
        case "command_denied":
        case "command_conflict":
        case "command_cancelled":
        case "command_timed_out":
        case "command_failed":
          commandRenderer?.flush();
          commandRenderer = undefined;
          io.write(formatCommandTerminal(event.type, event.message));
          break;
      }
    }
  } catch (error: unknown) {
    io.write(`\n${formatProviderFailure(describeError(error))}`);
  } finally {
    promptFinished = true;
    if (busyStarted && inputQueue !== undefined) {
      inputQueue.cancelPendingAsk();
    }
    if (busyStarted) {
      io.write(PROMPT);
    }
    await busy;
    controls.endPrompt();
  }
}

function isCommandTool(toolName: string): boolean {
  return toolName === "process.run";
}

/**
 * While a provider-accessible command runs, keep reading the terminal so
 * `/cancel` and Ctrl+C work. Type-ahead input is buffered and replayed after
 * the command completes; the final line read after completion is preserved so
 * no user input is ever lost. With an InputQueue, the in-flight read is
 * cancellable and the queue discards it when the command finishes.
 */
async function runBusyInput(
  io: SessionIO,
  controller: AbortController,
  inputBuffer: string[],
  isDone: () => boolean,
  inputQueue?: InputQueue,
): Promise<void> {
  for (;;) {
    const outcome =
      inputQueue === undefined
        ? { kind: "answer" as const, value: await io.ask("") }
        : await inputQueue.ask("", { signal: controller.signal });
    if (outcome.kind === "aborted" || outcome.kind === "discarded") {
      return;
    }
    if (outcome.kind !== "answer") {
      return;
    }
    const line = outcome.value;
    if (line === null) {
      controller.abort();
      return;
    }
    if (line.trim() === "/cancel" && !controller.signal.aborted) {
      controller.abort();
      io.write(formatCancelReport());
      return;
    }
    inputBuffer.push(line);
    if (isDone()) {
      return;
    }
  }
}

interface CommandOutputRenderer {
  stdout(text: string): void;
  stderr(text: string): void;
  flush(): void;
}

function createCommandOutputRenderer(write: (text: string) => void): CommandOutputRenderer {
  let partial: { readonly stream: "stdout" | "stderr"; readonly text: string } | null = null;
  const writeLine = (stream: "stdout" | "stderr", line: string): void => {
    write(`  [${stream}] ${sanitizeForDisplay(line)}\n`);
  };
  const emit = (stream: "stdout" | "stderr", text: string): void => {
    const segments = text.split("\n");
    if (partial !== null) {
      segments[0] = partial.text + (segments[0] ?? "");
      partial = null;
    }
    const last = segments[segments.length - 1] ?? "";
    if (last.length > 0) {
      partial = { stream, text: last };
    }
    for (let index = 0; index < segments.length - 1; index += 1) {
      writeLine(stream, segments[index] ?? "");
    }
  };
  return {
    stdout(text: string): void {
      emit("stdout", text);
    },
    stderr(text: string): void {
      emit("stderr", text);
    },
    flush(): void {
      if (partial !== null) {
        writeLine(partial.stream, partial.text);
        partial = null;
      }
    },
  };
}
