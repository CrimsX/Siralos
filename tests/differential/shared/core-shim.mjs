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
// R13.3 reference/research parity probes: the adapter modules import these
// values from `@siralos/core`; each aliases its real source module.
export { canonicalizeJson, sha256Hex } from "../../../packages/core/src/godot/digest.js";
export { normalizeRepositoryOrigin } from "../../../packages/core/src/reference/reference-declaration.js";
export {
  computeResearchDocumentContentDigest,
  computeResearchDocumentId,
} from "../../../packages/core/src/research/research-model.js";
export { VALIDATION_OFFLINE_PROFILE } from "../../../packages/core/src/security/profile.js";
export {
  REFERENCE_LIMITS,
  validateReferenceAlias,
} from "../../../packages/core/src/reference/reference-model.js";
// R8 Godot parity probes: the discovery profiler, knowledge,
// diagnostics, and LSP services import these values from
// `@siralos/core`; each aliases its real source module.
export {
  classifyGodotEdition,
  classifyGodotSupport,
} from "../../../packages/core/src/godot/engine-profile.js";
export {
  classifyGodotReleaseChannel,
  parseDeclaredVersion,
} from "../../../packages/core/src/godot/version.js";
export {
  GODOT_SELECTION_RANKS,
  rankGodotCandidates,
} from "../../../packages/core/src/godot/selection.js";
export { assessGodotCompatibility } from "../../../packages/core/src/godot/compatibility.js";
export {
  computeGodotCheckOnlyCommandDigest,
  computeGodotPreparedCheckDigest,
  createPreparedGDScriptCheck,
} from "../../../packages/core/src/godot/gdscript.js";
export { GODOT_DIAGNOSTICS_OFFLINE_PROFILE } from "../../../packages/core/src/security/profile.js";
export {
  KNOWLEDGE_SCHEMA_VERSION,
  classifyGodotManualChannel,
} from "../../../packages/core/src/godot/knowledge.js";
export { computeGDScriptPreparedSessionDigest } from "../../../packages/core/src/godot/lsp.js";
export { createPreparedGDScriptSession } from "../../../packages/core/src/godot/lsp.js";
export { GODOT_LSP_LOCAL_PROFILE } from "../../../packages/core/src/security/profile.js";
export { computeGodotRiskManifestDigest } from "../../../packages/core/src/godot/probe.js";
export { godotSymbolId } from "../../../packages/core/src/godot/api.js";
export { isBalancedText } from "../../../packages/core/src/godot/scene/text.js";
export { parseGodotVariant } from "../../../packages/core/src/godot/scene/variant.js";
