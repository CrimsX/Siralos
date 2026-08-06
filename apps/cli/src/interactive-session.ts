import type { SolarisApplication } from "@solaris/core";
import { parseInput } from "./input/parse-input.js";
import {
  describeError,
  formatHelp,
  formatInvalidCommand,
  formatProviderFailure,
  formatStatus,
  formatToolCancelled,
  formatToolCompleted,
  formatToolFailed,
  formatToolStarted,
  sanitizeForDisplay,
} from "./output.js";

export interface SessionIO {
  ask(prompt: string): Promise<string | null>;
  write(text: string): void;
  clear(): void;
}

const PROMPT = "> ";

export async function runInteractiveSession(
  io: SessionIO,
  application: SolarisApplication,
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
            io.write(formatStatus(application.getStatus()));
            break;
          case "clear":
            io.clear();
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
