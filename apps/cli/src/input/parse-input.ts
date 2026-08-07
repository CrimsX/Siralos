export type SlashCommand =
  "help" | "status" | "clear" | "tools" | "sandbox" | "permissions" | "exit";

export type ParsedInput =
  | {
      readonly type: "prompt";
      readonly text: string;
    }
  | {
      readonly type: "command";
      readonly command: SlashCommand;
    }
  | {
      readonly type: "empty";
    }
  | {
      readonly type: "invalid_command";
      readonly input: string;
    };

const SLASH_COMMANDS: readonly SlashCommand[] = [
  "help",
  "status",
  "clear",
  "tools",
  "sandbox",
  "permissions",
  "exit",
];

export function parseInput(raw: string): ParsedInput {
  const trimmed = raw.trim();
  if (trimmed.length === 0) {
    return { type: "empty" };
  }
  if (trimmed.startsWith("/")) {
    const command = findSlashCommand(trimmed);
    if (command !== null) {
      return { type: "command", command };
    }
    return { type: "invalid_command", input: trimmed };
  }
  return { type: "prompt", text: trimmed };
}

function findSlashCommand(text: string): SlashCommand | null {
  return SLASH_COMMANDS.find((command) => `/${command}` === text) ?? null;
}
