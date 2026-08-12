import type { ConfigurationDiagnosticResult } from "@siralos/core";
import { readFile, lstat } from "node:fs/promises";
import { loadUserConfig } from "./user-config.js";

/**
 * Read-only configuration diagnostics (Stage 3 milestone 6), shared by the
 * CLI composition root and the final-boundary behavior/effect tests: the
 * doctor's configuration source reuses `loadUserConfig` as the single
 * validator (never duplicating schema logic) and derives section presence
 * from one raw read. Credential values never enter the result — only env
 * variable NAMES with referenced/present booleans (this runtime references
 * no provider credentials at all).
 */

interface RawConfigShape {
  readonly sandbox?: Record<string, unknown>;
  readonly godot?: Record<string, unknown>;
  readonly quality?: Record<string, unknown>;
  readonly references?: Record<string, unknown>;
}

export async function readConfigurationDiagnostics(
  configPath: string,
): Promise<ConfigurationDiagnosticResult> {
  const sections = [
    { name: "sandbox", present: false },
    { name: "godot", present: false },
    { name: "quality", present: false },
    { name: "references", present: false },
  ];
  let raw: RawConfigShape = {};
  try {
    const content = await readFile(configPath, "utf8");
    const parsed = JSON.parse(content) as unknown;
    if (typeof parsed === "object" && parsed !== null) {
      raw = parsed;
    }
  } catch {
    // Readability/validity are reported by the checks below; here we only
    // derive section presence from whatever parsed.
  }
  for (const section of sections) {
    section.present = raw[section.name as keyof RawConfigShape] !== undefined;
  }
  // `loadUserConfig` is the single schema validator: it rejects unknown
  // sections/fields and unreadable or symlinked files with precise
  // messages, which surface in validationErrors.
  let validationErrors: string[] = [];
  let loaded = true;
  try {
    await loadUserConfig(configPath);
  } catch (error: unknown) {
    loaded = false;
    validationErrors = [error instanceof Error ? error.message : String(error)];
  }
  return {
    loaded,
    sections,
    // The loader rejects unknown fields outright; there is no separate
    // unknown-fields channel in the real wiring.
    unknownFields: [],
    validationErrors,
    // No provider credential environment variables exist in this runtime
    // (the only provider is the deterministic fake; no model routes).
    credentialRefs: [],
    overrideInUse: false,
  };
}

export type ConfigurationFileState = "readable" | "missing" | "unreadable";

/**
 * Configuration-file state for the runtime area. Uses lstat (no symlink
 * following) so a symlinked config file is reported unreadable — matching
 * `loadUserConfig`, which rejects symlinks.
 */
export async function readConfigurationFileState(
  configPath: string,
): Promise<ConfigurationFileState> {
  try {
    const stats = await lstat(configPath);
    return stats.isFile() ? "readable" : "unreadable";
  } catch {
    return "missing";
  }
}
