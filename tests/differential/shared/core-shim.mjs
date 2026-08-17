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
export { GODOT_LIMITS } from "../../../packages/core/src/godot/limits.js";
export { normalizeDefinitionLocations } from "../../../packages/core/src/language/definition.js";
export { normalizeDiagnosticPayload } from "../../../packages/core/src/language/diagnostic.js";
export { sanitizeControlCharacters } from "../../../packages/core/src/language/sanitize.js";
export { toOneBasedRange } from "../../../packages/core/src/language/position.js";
export { truncateUtf8Bytes } from "../../../packages/core/src/language/truncate.js";
export { isProtectedBehavioralConfigPath } from "../../../packages/core/src/security/behavioral-config.js";
export { GitError } from "../../../packages/core/src/git/git-errors.js";
export { VALIDATION_OFFLINE_PROFILE } from "../../../packages/core/src/security/profile.js";
export { REFERENCE_LIMITS, validateReferenceAlias } from "../../../packages/core/src/reference/reference-model.js";
