/**
 * Oracle-only re-export shim for the `@siralos/core` package specifier.
 *
 * The R4 probes exercise adapter modules that import selected VALUES
 * from `@siralos/core`. The full source index uses TypeScript syntax
 * (constructor parameter properties) that strip-only mode cannot load,
 * so this harness shim re-exports exactly those values from their real
 * source modules. Type-only imports are erased by the type stripper and
 * need no shim. The shim aliases modules; it never reimplements behavior.
 */
export { buildWorkspaceSummary } from "../../../packages/core/src/workspace/workspace-summary.js";
export { extractGDScriptStructure } from "../../../packages/core/src/workspace/gdscript-structure.js";
export { isWorkspaceReadMode } from "../../../packages/core/src/workspace/workspace-read-mode.js";
export { isProtectedBehavioralConfigPath } from "../../../packages/core/src/security/behavioral-config.js";
export { GitError } from "../../../packages/core/src/git/git-errors.js";
export { VALIDATION_OFFLINE_PROFILE } from "../../../packages/core/src/security/profile.js";
