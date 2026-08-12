import {
  GODOT_KNOWN_OPTIONS,
  createEmptyGodotCommandCapabilities,
  type GodotCommandCapabilities,
} from "@siralos/core";

export interface GodotHelpParseResult {
  readonly capabilities: GodotCommandCapabilities;
  /** Count of unrecognized option tokens (bounded diagnostic only). */
  readonly unknownOptionCount: number;
}

const OPTION_TOKEN_PATTERN = /--[a-z][a-z0-9-]*/g;

/** Recognized options that are not capabilities (e.g. `--help` itself). */
const KNOWN_NON_CAPABILITY_OPTIONS: readonly string[] = ["--help"];

/**
 * Extracts command capabilities from bounded `--help` output.
 *
 * Complete option tokens are matched exactly against the fixed known set;
 * substrings never match, malformed output never creates false
 * capabilities, and unrecognized options are preserved only as a bounded
 * diagnostic count. Advertised support is not operational support.
 */
export function parseHelpCapabilities(helpText: string): GodotHelpParseResult {
  const values: { -readonly [K in keyof GodotCommandCapabilities]: boolean } = {
    ...createEmptyGodotCommandCapabilities(),
  };
  const knownByOption = new Map(
    GODOT_KNOWN_OPTIONS.map((entry) => [entry.option, entry.capability]),
  );
  const nonCapabilityOptions = new Set(KNOWN_NON_CAPABILITY_OPTIONS);
  const seen = new Set<string>();
  let unknownOptionCount = 0;
  for (const match of helpText.matchAll(OPTION_TOKEN_PATTERN)) {
    const token = match[0];
    if (seen.has(token)) {
      continue;
    }
    seen.add(token);
    const capability = knownByOption.get(token);
    if (capability === undefined) {
      if (!nonCapabilityOptions.has(token)) {
        unknownOptionCount += 1;
      }
      continue;
    }
    values[capability] = true;
  }
  return { capabilities: values, unknownOptionCount };
}
