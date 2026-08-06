import type { SessionStatus, ToolDefinition } from "@solaris/core";

export function formatHeader(providerId: string): string {
  return `Solaris
Interactive Godot development harness
Provider: ${providerId}
`;
}

export function formatHelp(): string {
  return `Available commands:
  /help    Show this help
  /status  Show provider, session, and workspace status
  /clear   Clear the terminal (conversation is kept)
  /tools   List the available tools
  /exit    Close Solaris
`;
}

export function formatStatus(
  status: SessionStatus,
  workspaceRoot: string,
  toolCount: number,
): string {
  const sessionState = status.state === "responding" ? "responding" : "active";
  return `Provider: ${status.providerId}
Session: ${sessionState}
Messages: ${status.messageCount}
Workspace: ${workspaceRoot}
Tools: ${toolCount}
`;
}

export function formatTools(tools: readonly ToolDefinition[]): string {
  if (tools.length === 0) {
    return "Available tools:\n  (none)\n";
  }
  const lines = tools.map((tool) => `  ${tool.name} - ${tool.description} (read-only)`);
  return `Available tools:\n${lines.join("\n")}\n`;
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
