import type { SolarisApplication, SolarisSecurity, ToolDefinition } from "@solaris/core";
import { parseInput } from "./input/parse-input.js";
import {
  describeError,
  formatHelp,
  formatInvalidCommand,
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
  sanitizeForDisplay,
} from "./output.js";

export interface SessionIO {
  ask(prompt: string): Promise<string | null>;
  write(text: string): void;
  clear(): void;
}

export interface SessionInfo {
  readonly workspaceRoot: string;
  readonly tools: readonly ToolDefinition[];
  readonly security: SolarisSecurity;
}

const PROMPT = "> ";

export async function runInteractiveSession(
  io: SessionIO,
  application: SolarisApplication,
  sessionInfo: SessionInfo,
): Promise<number> {
  for (;;) {
    const input = await io.ask(PROMPT);
    if (input === null) {
      return 0;
    }
    const parsed = parseInput(input);
    switch (parsed.type) {
      case "prompt":
        await runPrompt(io, application, parsed.text);
        break;
      case "command":
        switch (parsed.command) {
          case "help":
            io.write(formatHelp());
            break;
          case "status":
            io.write(
              formatStatus(
                application.getStatus(),
                sessionInfo.workspaceRoot,
                sessionInfo.tools.length,
                sessionInfo.security.profile.id,
              ),
            );
            break;
          case "clear":
            io.clear();
            break;
          case "tools":
            io.write(formatTools(sessionInfo.tools));
            break;
          case "sandbox":
            await runSandboxCheck(io, sessionInfo.security);
            break;
          case "permissions":
            io.write(
              formatPermissions(sessionInfo.security.policy, sessionInfo.security.profile.id),
            );
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
): Promise<void> {
  try {
    for await (const event of application.sendPrompt(text)) {
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
          io.write(formatToolStarted(event.toolName, sanitizeForDisplay(event.displayInput)));
          break;
        case "tool_completed":
          io.write(formatToolCompleted(sanitizeForDisplay(event.summary)));
          break;
        case "tool_failed":
          io.write(formatToolFailed(sanitizeForDisplay(event.message)));
          break;
        case "tool_cancelled":
          io.write(formatToolCancelled());
          break;
      }
    }
  } catch (error: unknown) {
    io.write(`\n${formatProviderFailure(describeError(error))}`);
  }
}
