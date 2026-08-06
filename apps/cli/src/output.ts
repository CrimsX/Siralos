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

export function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unexpected error occurred.";
}
