import type { SafeDiagnostic } from "./diagnostics.js";

/** Static, non-authoritative autoload entry extracted from project.godot. */
export interface GodotAutoloadSummary {
  readonly name: string;
  /** Raw resource target (`res://...` or `*res://...` for singletons). */
  readonly target: string;
  readonly isSingleton: boolean;
}

/** Editor plugin descriptor extracted from a bounded `plugin.cfg` file. */
export interface GodotPluginSummary {
  /** Workspace-relative plugin directory, e.g. `addons/example`. */
  readonly path: string;
  readonly name: string;
  readonly description: string;
  readonly author: string;
  readonly version: string;
  readonly scriptPath: string;
  readonly language: "gdscript" | "dotnet" | "unknown";
  /** Whether the project declares the plugin enabled. */
  readonly enabled: boolean;
  /** Heuristic only: likely an import plugin based on script inventory. */
  readonly importPluginHeuristic: boolean;
}

/** GDExtension descriptor extracted from a bounded `.gdextension` file. */
export interface GodotGDExtensionSummary {
  /** Workspace-relative descriptor path. */
  readonly path: string;
  readonly compatibilityMinimum: string | null;
  /** Library target paths from the descriptor (workspace-relative when inside). */
  readonly libraryTargets: readonly string[];
  /** Whether any referenced native library file exists. */
  readonly libraryFilesExist: boolean;
  /** Whether any target path escapes through symbolic links. */
  readonly escapesThroughSymlinks: boolean;
}

/**
 * Statically identified project components that may execute when the editor
 * or project is opened. This inventory informs the future recovery-mode
 * trust decision; nothing here is ever loaded or run.
 */
export interface GodotExecutableContentInventory {
  /** Workspace-relative paths of `.gd` files with a lexical `@tool` marker. */
  readonly toolScripts: readonly string[];
  readonly editorPlugins: readonly GodotPluginSummary[];
  /** Heuristic import-plugin findings (workspace-relative script paths). */
  readonly importPlugins: readonly string[];
  readonly gdextensionDescriptors: readonly GodotGDExtensionSummary[];
  readonly autoloadCount: number;
  readonly dotnetProjectFiles: readonly string[];
  readonly scanTruncated: boolean;
}

export function createEmptyGodotExecutableContentInventory(): GodotExecutableContentInventory {
  return {
    toolScripts: [],
    editorPlugins: [],
    importPlugins: [],
    gdextensionDescriptors: [],
    autoloadCount: 0,
    dotnetProjectFiles: [],
    scanTruncated: false,
  };
}

export type GodotLanguageProfile = "gdscript" | "dotnet" | "mixed" | "unknown";

/**
 * Static project profile. Every value is derived from untrusted project
 * files without executing anything; results are non-authoritative.
 */
export interface GodotProjectProfile {
  readonly detected: boolean;
  readonly projectFileSha256: string | null;
  readonly configVersion: number | null;
  readonly name: string | null;
  readonly applicationVersion: string | null;
  readonly declaredFeatures: readonly string[];
  /** Declared engine feature parsed from `config/features` (e.g. `4.7`). */
  readonly declaredEngineVersion: {
    major: number;
    minor: number;
    patch: number | null;
    raw: string;
  } | null;
  readonly mainScene: string | null;
  /** Whether the main scene file exists; null when no main scene is declared. */
  readonly mainSceneExists: boolean | null;
  readonly mainSceneIsSymlink: boolean;
  readonly renderingMethods: readonly string[];
  readonly languageProfile: GodotLanguageProfile;
  readonly autoloads: readonly GodotAutoloadSummary[];
  readonly enabledEditorPlugins: readonly string[];
  readonly executableContent: GodotExecutableContentInventory;
  readonly warnings: readonly SafeDiagnostic[];
}

export function createEmptyGodotProjectProfile(): GodotProjectProfile {
  return {
    detected: false,
    projectFileSha256: null,
    configVersion: null,
    name: null,
    applicationVersion: null,
    declaredFeatures: [],
    declaredEngineVersion: null,
    mainScene: null,
    mainSceneExists: null,
    mainSceneIsSymlink: false,
    renderingMethods: [],
    languageProfile: "unknown",
    autoloads: [],
    enabledEditorPlugins: [],
    executableContent: createEmptyGodotExecutableContentInventory(),
    warnings: [],
  };
}
