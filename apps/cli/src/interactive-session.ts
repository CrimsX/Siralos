import type { SiralosApplication, ProjectionMode, TaskRuntime } from "@siralos/core";
import { parseInput } from "./input/parse-input.js";
import type { InputQueue } from "./input/input-queue.js";
import type { SessionControls, SessionIO, SessionInfo } from "./session/session-types.js";
import { buildSessionStatusView } from "./session/session-status.js";
import {
  runCommandsCommand,
  runDiffCommand,
  runGitStatusCommand,
  runSandboxCheck,
  runUndoCommand,
} from "./session/session-system-commands.js";
import {
  runGDScriptCheckCommand,
  runGDScriptDiagnosticsCommand,
  runGDScriptLSPCommand,
  runGDScriptLSPStopCommand,
  runGDScriptPositionCommand,
  runGodotApiCommand,
  runGodotCommand,
  runGodotDoctorCommand,
  runGodotInstallationsCommand,
  runGodotKnowledgeRefreshCommand,
  runGodotProbeCommand,
  runGodotProjectCommand,
} from "./session/session-godot-commands.js";
import {
  hasActiveDevelopmentTaskFlow,
  runCancelCommand,
  runDevelopCommand,
  runPlanCommand,
  runReadStructureCommand,
  runReviewChangeCommand,
  runTaskCommand,
  runTaskStatusCommand,
} from "./session/session-development-commands.js";
import { runBriefCommand, runMilestoneCommand } from "./session/session-briefing-commands.js";
export type { SessionControls, SessionIO, SessionInfo } from "./session/session-types.js";
import { createCliDoctor, isDoctorArea, type CliDoctorDependencies } from "./bootstrap/doctor.js";
import {
  describeError,
  formatCancelReport,
  formatCheckpoints,
  formatCommandCompleted,
  formatCommandStarted,
  formatCommandTerminal,
  formatContextStatus,
  formatPlanningStatus,
  formatInstructions,
  formatKnowledge,
  formatKnowledgeTrace,
  formatReferences,
  formatReferenceDetail,
  formatResearchStatus,
  formatDevelopmentStatus,
  formatGodotKnowledgeStatus,
  formatGodotProbeStatus,
  formatHelp,
  formatInvalidCommand,
  formatPermissions,
  formatProviderFailure,
  formatQualityReport,
  formatStatus,
  formatSiralosDoctorReport,
  formatSelfReference,
  formatToolCancelled,
  formatToolCompleted,
  formatToolProjection,
  formatToolFailed,
  formatDomains,
  formatTools,
  formatToolStarted,
  sanitizeForDisplay,
  sanitizePathForDisplay,
} from "./output.js";

const PROMPT = "> ";

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
  application: SiralosApplication,
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
        await runPrompt(
          io,
          application,
          parsed.text,
          controls,
          inputBuffer,
          inputQueue,
          sessionInfo.tasks,
        );
        break;
      case "command":
        switch (parsed.command) {
          case "help":
            io.write(formatHelp());
            break;
          case "status":
            io.write(formatStatus(await buildSessionStatusView(application, sessionInfo)));
            io.write(formatPlanningStatus(sessionInfo.tasks.latestTask()?.snapshot() ?? null));
            break;
          case "clear":
            io.clear();
            break;
          case "tools":
            io.write(formatTools(sessionInfo.tools, sessionInfo.security));
            io.write(formatToolProjection(sessionInfo.projection));
            break;
          case "domains":
            io.write(formatDomains());
            break;
          case "domains-add":
            io.write("Domains: /domains-add requires the Add Plugin selection flow.\n");
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
          case "develop":
            await runDevelopCommand(io, sessionInfo, controls, parsed.args, (request) =>
              runPrompt(
                io,
                application,
                request,
                controls,
                inputBuffer,
                inputQueue,
                sessionInfo.tasks,
                { mode: "development" },
              ),
            );
            break;
          case "plan":
            await runPlanCommand(io, sessionInfo, controls, parsed.args);
            break;
          case "development-status":
            io.write(formatDevelopmentStatus(sessionInfo.development.status()));
            io.write(formatPlanningStatus(sessionInfo.tasks.latestTask()?.snapshot() ?? null));
            io.write(formatContextStatus(sessionInfo.projection));
            break;
          case "task":
            runTaskCommand(io, sessionInfo, parsed.args);
            break;
          case "task-status":
            io.write(runTaskStatusCommand(sessionInfo));
            break;
          case "context":
            io.write(formatContextStatus(sessionInfo.projection));
            break;
          case "instructions":
            io.write(
              formatInstructions(
                sessionInfo.instructions.instructions(),
                sessionInfo.instructions.revision(),
              ),
            );
            break;
          case "references":
            io.write(
              formatReferences(
                sessionInfo.references,
                sessionInfo.referenceMaterializer,
                sessionInfo.referenceConfigError,
              ),
            );
            break;
          case "reference":
            runReferenceCommand(io, sessionInfo, parsed.args);
            break;
          case "research-status":
            io.write(
              formatResearchStatus(
                sessionInfo.research,
                sessionInfo.security,
                sessionInfo.researchSources,
              ),
            );
            break;
          case "doctor":
            await runSiralosDoctorCommand(io, sessionInfo, parsed.args);
            break;
          case "siralos":
            io.write(formatSelfReference(sessionInfo.selfReference));
            break;
          case "knowledge":
            if (parsed.args[0] === "why") {
              io.write(formatKnowledgeTrace(sessionInfo.projectKnowledge.lastRetrievalTrace()));
            } else {
              io.write(formatKnowledge(sessionInfo.projectKnowledge));
            }
            break;
          case "read-structure":
            await runReadStructureCommand(io, sessionInfo, parsed.args);
            break;
          case "brief":
            runBriefCommand(io, sessionInfo);
            break;
          case "milestone":
            runMilestoneCommand(io, sessionInfo);
            break;
          case "quality":
            io.write(formatQualityReport(sessionInfo.development.qualityReport()));
            break;
          case "review-change":
            await runReviewChangeCommand(io, sessionInfo, controls);
            break;
          case "cancel":
            await runCancelCommand(io, sessionInfo, controls);
            break;
          case "exit":
            return 0;
          default: {
            // Exhaustiveness guard: the switch cannot drift from the
            // command catalog — a catalogued command without a case here
            // fails to compile (parsed.command would not be `never`).
            const exhaustive: never = parsed.command;
            void exhaustive;
            io.write("Unhandled command.\n");
          }
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

function runReferenceCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  args: readonly string[],
): void {
  const selector = args.join(" ").trim();
  if (selector.length === 0) {
    io.write("Usage: /reference <alias>\n");
    return;
  }
  io.write(
    formatReferenceDetail(sessionInfo.references, sessionInfo.referenceMaterializer, selector),
  );
}

async function runSiralosDoctorCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  args: readonly string[],
): Promise<void> {
  const jsonOutput = args.includes("--json");
  const areaArg = args.find((arg) => !arg.startsWith("--"));
  if (areaArg !== undefined && !isDoctorArea(areaArg)) {
    io.write(
      `Unknown doctor area: ${areaArg}. Areas: runtime, configuration, providers, sandbox, workspace, godot, project, references, research, capabilities.\n`,
    );
    return;
  }
  const doctor = createCliDoctor(
    sessionDoctorDependencies(
      sessionInfo,
      hasActiveDevelopmentTaskFlow() ? "development" : "generic",
    ),
  );
  try {
    const report = await doctor.inspect({
      ...(areaArg === undefined ? {} : { areas: [areaArg] }),
    });
    if (jsonOutput) {
      io.write(`${JSON.stringify(report, null, 2)}\n`);
    } else {
      io.write(formatSiralosDoctorReport(report));
    }
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeError(error)));
  }
}

function sessionDoctorDependencies(sessionInfo: SessionInfo, mode: string): CliDoctorDependencies {
  return {
    workspaceRoot: sessionInfo.workspaceRoot,
    configPath: sessionInfo.configPath,
    policy: sessionInfo.policy,
    profile: sessionInfo.profile,
    sandbox: sessionInfo.sandbox,
    provider: sessionInfo.provider,
    godot: sessionInfo.godot,
    references: sessionInfo.references,
    referenceConfigError: sessionInfo.referenceConfigError,
    research: sessionInfo.research,
    researchSources: sessionInfo.researchSources,
    tasks: sessionInfo.tasks,
    taskSources: sessionInfo.taskSources,
    git: sessionInfo.git,
    checkpoints: sessionInfo.checkpoints,
    tools: sessionInfo.tools,
    mode,
  };
}

async function runPrompt(
  io: SessionIO,
  application: SiralosApplication,
  text: string,
  controls: SessionControls,
  inputBuffer: string[],
  inputQueue?: InputQueue,
  tasks?: TaskRuntime,
  options?: { readonly mode?: ProjectionMode },
): Promise<void> {
  const controller = controls.beginPrompt();
  let busy: Promise<void> | undefined;
  let busyStarted = false;
  let promptFinished = false;
  let commandRenderer: CommandOutputRenderer | undefined;
  try {
    for await (const event of application.sendPrompt(text, controller.signal, options)) {
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
          tasks?.latestTask()?.observe({
            action: `tool.${event.toolName}`,
            fingerprint: `${event.toolName}:${sanitizeForDisplay(event.summary)}`,
          });
          break;
        case "tool_failed":
          if (!isCommandTool(event.toolName)) {
            io.write(formatToolFailed(sanitizeForDisplay(event.message)));
          }
          tasks?.latestTask()?.observe({
            action: `tool.${event.toolName}`,
            fingerprint: `${event.toolName}:${sanitizeForDisplay(event.message)}`,
          });
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
        case "context_pressure":
          if (event.state !== "normal") {
            io.write(
              `  \u26A0 context pressure ${event.state}: ${event.estimatedTokens} est. tokens / ${event.workingMaximum} working\n`,
            );
          }
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
