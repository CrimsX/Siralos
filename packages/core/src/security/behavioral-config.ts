/**
 * Protected behavioral configuration (Stage 3 milestone 4).
 *
 * Behavioral configuration files receive stricter mutation treatment than
 * ordinary source: they define how Solaris and the model behave, so a
 * normal `workspace.write` grant must never implicitly cover them.
 * Behavioral modification requires its own host authorization path, which
 * is not offered at this stage — every mutation surface fails closed
 * before any write, approval, or checkpoint.
 *
 * Protected surfaces included here map to actual repository conventions:
 *
 *   AGENTS.md        — project instruction files at any workspace depth
 *   .solaris/**      — Solaris behavioral configuration directory
 *
 * Future surfaces (workflow definitions, skills, agent definitions,
 * plugin/MCP configuration) slot into the same classifier when they
 * exist; they are deliberately not invented here.
 */

export const BEHAVIORAL_CONFIG_DIRECTORY = ".solaris";
export const BEHAVIORAL_INSTRUCTION_FILE = "AGENTS.md";

/**
 * True when the workspace-relative path is protected behavioral
 * configuration. Pure and deterministic: the same classifier is used by
 * the core change-set validator and by the adapter write-path guards, so
 * no mutation surface can disagree about the boundary.
 */
export function isProtectedBehavioralConfigPath(workspaceRelativePath: string): boolean {
  const normalized = workspaceRelativePath.replace(/\\/g, "/").replace(/^\.\//, "");
  if (normalized.length === 0 || normalized === ".") {
    return false;
  }
  const components = normalized.split("/").filter((component) => component.length > 0);
  const basename = components.at(-1) ?? "";
  if (components.some((component) => component.toLowerCase() === BEHAVIORAL_CONFIG_DIRECTORY)) {
    return true;
  }
  return basename.toLowerCase() === BEHAVIORAL_INSTRUCTION_FILE.toLowerCase();
}

/** The protected paths among a set of workspace-relative paths. */
export function classifyBehavioralConfigPaths(paths: readonly string[]): readonly string[] {
  return paths.filter((path) => isProtectedBehavioralConfigPath(path));
}
