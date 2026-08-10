import type { CommandId } from "@solaris/core";
import { COMMAND_CATALOG_IDS } from "@solaris/core";

/**
 * The interactive command vocabulary derives from the core command catalog
 * (Stage 3 milestone 6): a command cannot be typed in this session unless
 * it is catalogued, and the exhaustive switch in the session renderer
 * cannot compile for an id the parser cannot produce. The catalog is also
 * what the built-in @solaris self-reference documents — no hand-maintained
 * command list can drift.
 */
export type SlashCommand = CommandId;

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

export const SLASH_COMMANDS: readonly SlashCommand[] =
  COMMAND_CATALOG_IDS as readonly SlashCommand[];

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
