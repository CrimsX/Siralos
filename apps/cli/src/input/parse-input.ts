export type SlashCommand =
  | "help"
  | "status"
  | "clear"
  | "tools"
  | "sandbox"
  | "permissions"
  | "git-status"
  | "diff"
  | "checkpoints"
  | "undo"
  | "commands"
  | "cancel"
  | "develop"
  | "development-status"
  | "quality"
  | "review-change"
  | "godot"
  | "godot-installations"
  | "godot-project"
  | "godot-doctor"
  | "godot-probe"
  | "godot-probe-status"
  | "godot-knowledge"
  | "godot-knowledge-refresh"
  | "godot-api"
  | "gdscript-check"
  | "gdscript-diagnostics"
  | "gdscript-lsp"
  | "gdscript-lsp-stop"
  | "gdscript-hover"
  | "gdscript-complete"
  | "gdscript-definition"
  | "exit";

export type ParsedInput =
  | {
      readonly type: "prompt";
      readonly text: string;
    }
  | {
      readonly type: "command";
      readonly command: SlashCommand;
      readonly args: readonly string[];
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
  "git-status",
  "diff",
  "checkpoints",
  "undo",
  "commands",
  "cancel",
  "develop",
  "development-status",
  "quality",
  "review-change",
  "godot",
  "godot-installations",
  "godot-project",
  "godot-doctor",
  "godot-probe",
  "godot-probe-status",
  "godot-knowledge",
  "godot-knowledge-refresh",
  "godot-api",
  "gdscript-check",
  "gdscript-diagnostics",
  "gdscript-lsp",
  "gdscript-lsp-stop",
  "gdscript-hover",
  "gdscript-complete",
  "gdscript-definition",
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
      return { type: "command", command, args: extractArgs(trimmed) };
    }
    return { type: "invalid_command", input: trimmed };
  }
  return { type: "prompt", text: trimmed };
}

function findSlashCommand(text: string): SlashCommand | null {
  return SLASH_COMMANDS.find((command) => `/${command}` === text.split(/\s+/)[0]) ?? null;
}

function extractArgs(text: string): readonly string[] {
  const parts = text.split(/\s+/).slice(1);
  return parts.filter((part) => part.length > 0);
}
