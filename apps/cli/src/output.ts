import type { SessionStatus } from "@solaris/core";

export function formatHeader(providerId: string): string {
  return `Solaris
Interactive Godot development harness
Provider: ${providerId}
`;
}

export function formatHelp(): string {
  return `Available commands:
  /help    Show this help
  /status  Show provider and session status
  /clear   Clear the terminal (conversation is kept)
  /exit    Close Solaris
`;
}

export function formatStatus(status: SessionStatus): string {
  const sessionState = status.state === "responding" ? "responding" : "active";
  return `Provider: ${status.providerId}
Session: ${sessionState}
Messages: ${status.messageCount}
`;
}

export function formatInvalidCommand(input: string): string {
  return `Unknown command: ${input}
Type /help for the list of available commands.
`;
}

export function formatProviderFailure(message: string): string {
  return `[error] ${message}
`;
}

export function formatToolStarted(toolName: string, displayInput: string): string {
  return `\u25CF ${toolName} ${displayInput}
`;
}

export function formatToolCompleted(summary: string): string {
  return `  ${summary}
`;
}

export function formatToolFailed(message: string): string {
  return `  \u2715 ${message}
`;
}

export function formatToolCancelled(): string {
  return "  \u2715 cancelled\n";
}

const CONTROL_CHARACTER_PATTERN = new RegExp(`[\u0000-\u001f\u007f]`, "g");

export function sanitizeForDisplay(text: string): string {
  return text.replace(CONTROL_CHARACTER_PATTERN, "\uFFFD");
}

export function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unexpected error occurred.";
}
