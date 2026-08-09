/**
 * Workspace read modes (Stage 3 milestone 3).
 *
 * exact      — authoritative source access; the only basis for text mutation
 * structural — deterministic syntax/project structure, no source bodies
 * summary    — bounded advisory exploration; never authoritative source
 */

export type WorkspaceReadMode = "exact" | "structural" | "summary";

export interface WorkspaceReadModes {
  readonly exact: "exact";
  readonly structural: "structural";
  readonly summary: "summary";
}

export const WORKSPACE_READ_MODES: readonly WorkspaceReadMode[] = [
  "exact",
  "structural",
  "summary",
];

export function isWorkspaceReadMode(value: unknown): value is WorkspaceReadMode {
  return value === "exact" || value === "structural" || value === "summary";
}
