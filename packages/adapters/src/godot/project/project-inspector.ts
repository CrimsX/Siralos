import {
  parseDeclaredVersion,
  GODOT_LIMITS,
  type GodotApplicationEvent,
  type GodotProjectProfile,
  type SafeDiagnostic,
} from "@siralos/core";
import { scanProjectFile } from "./project-scanner.js";
import { readProjectFile } from "./project-files.js";
import { detectLanguageProfile } from "./language-profile.js";
import { inventoryExecutableContent } from "./content-inventory.js";
import {
  DEFAULT_FS_OPS,
  validateProjectRelativePath,
  verifyProjectPathContainment,
  type GodotProjectFsOps,
} from "./traversal-limits.js";
import { join } from "node:path";

export interface GodotProjectInspectorDependencies {
  readonly workspaceRoot: string;
  readonly onEvent?: (event: GodotApplicationEvent) => void;
  /** Filesystem seam (lstat/realpath/readdir/open); tests spy on every call. */
  readonly fsOps?: GodotProjectFsOps;
}

export interface GodotProjectInspector {
  inspect(signal?: AbortSignal): Promise<GodotProjectProfile>;
}

/**
 * Static project inspection: root-only `project.godot` detection, a
 * conservative scanner, language-profile evidence, and an executable-content
 * inventory. Nothing is executed, imported, or mutated; all results are
 * static and non-authoritative.
 *
 * The workspace root is canonicalized once per inspection and every
 * project-controlled path value is lexically and canonically contained
 * before any filesystem access. Every inspection rescans the complete
 * bounded project: changes to `project.godot`, tool scripts, plugin
 * descriptors and plugin scripts, GDExtension descriptors, C# files, and
 * main-scene/resource references are always visible on the next inspection.
 * A stale-profile cache is never used, because no bounded digest of the
 * inventoried content could be proven complete against a same-user writer;
 * size and mtime are never trusted for security-relevant invalidation.
 */
export function createGodotProjectInspector(
  dependencies: GodotProjectInspectorDependencies,
): GodotProjectInspector {
  const fsOps = dependencies.fsOps ?? DEFAULT_FS_OPS;

  async function inspect(signal?: AbortSignal): Promise<GodotProjectProfile> {
    let canonicalRoot: string;
    try {
      canonicalRoot = await fsOps.realpath(dependencies.workspaceRoot);
    } catch {
      return notDetected(
        {
          severity: "warning",
          message: "The workspace root could not be resolved; project inspection is unavailable.",
        },
        dependencies,
      );
    }
    const read = await readProjectFile(canonicalRoot, signal, fsOps);
    if (!read.ok) {
      return notDetected(
        {
          severity: "info",
          message:
            read.reason === "missing"
              ? "No project.godot exists at the workspace root; this is not a detected Godot project."
              : read.message,
        },
        dependencies,
      );
    }
    const scan = scanProjectFile(read.content);
    const warnings: SafeDiagnostic[] = [...scan.warnings];
    if (scan.truncated) {
      warnings.push({
        severity: "warning",
        message: "The project.godot scan hit a bound and was truncated.",
      });
    }
    const declaredEngineVersion = extractDeclaredEngineVersion(scan.declaredFeatures);
    const mainScene = scan.mainScene;
    const mainSceneInfo = await inspectMainScene(canonicalRoot, mainScene, fsOps);
    if (mainSceneInfo !== null) {
      if (mainSceneInfo.unsafe) {
        warnings.push({
          severity: "warning",
          message: `The main scene ${mainScene} is not a contained project path and was not inspected.`,
        });
      } else if (mainSceneInfo.isSymlink) {
        warnings.push({
          severity: "warning",
          message: `The main scene ${mainScene} is a symbolic link.`,
        });
      }
    }
    const language = await detectLanguageProfile({
      workspaceRoot: canonicalRoot,
      ...(signal === undefined ? {} : { signal }),
      dotnetAssemblyName: scan.dotnetAssemblyName,
      declaredFeatures: scan.declaredFeatures,
    });
    for (const evidence of language.evidence) {
      warnings.push({ severity: "info", message: evidence });
    }
    if (language.truncated) {
      warnings.push({
        severity: "warning",
        message: "The language-profile traversal was truncated; the profile is uncertain.",
      });
    }
    const content = await inventoryExecutableContent({
      workspaceRoot: canonicalRoot,
      ...(signal === undefined ? {} : { signal }),
      enabledPlugins: scan.enabledPlugins,
      autoloadCount: scan.autoloads.length,
      fsOps,
    });
    warnings.push(...content.warnings);
    const profile: GodotProjectProfile = {
      detected: true,
      projectFileSha256: read.sha256,
      configVersion: scan.configVersion,
      name: scan.name,
      applicationVersion: scan.applicationVersion,
      declaredFeatures: scan.declaredFeatures,
      declaredEngineVersion,
      mainScene,
      mainSceneExists: mainSceneInfo === null ? null : mainSceneInfo.exists,
      mainSceneIsSymlink: mainSceneInfo?.isSymlink ?? false,
      renderingMethods: scan.renderingMethods,
      languageProfile: language.profile,
      autoloads: scan.autoloads,
      enabledEditorPlugins: scan.enabledPlugins,
      executableContent: content.inventory,
      warnings,
    };
    dependencies.onEvent?.({
      type: "godot_project_inspected",
      detected: true,
      warnings: warnings.length,
    });
    return profile;
  }

  return { inspect };
}

function notDetected(
  warning: SafeDiagnostic,
  dependencies: GodotProjectInspectorDependencies,
): GodotProjectProfile {
  const profile: GodotProjectProfile = {
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
    executableContent: {
      toolScripts: [],
      editorPlugins: [],
      importPlugins: [],
      gdextensionDescriptors: [],
      autoloadCount: 0,
      dotnetProjectFiles: [],
      scanTruncated: false,
      scanTruncationReason: "none",
    },
    warnings: [warning],
  };
  dependencies.onEvent?.({ type: "godot_project_inspected", detected: false, warnings: 0 });
  return profile;
}

function extractDeclaredEngineVersion(features: readonly string[]): {
  major: number;
  minor: number;
  patch: number | null;
  raw: string;
} | null {
  for (const feature of features) {
    const parsed = parseDeclaredVersion(feature);
    if (parsed !== null) {
      return parsed;
    }
  }
  return null;
}

async function inspectMainScene(
  canonicalRoot: string,
  mainScene: string | null,
  fsOps: GodotProjectFsOps,
): Promise<{
  readonly exists: boolean;
  readonly isSymlink: boolean;
  readonly unsafe: boolean;
} | null> {
  if (mainScene === null) {
    return null;
  }
  if (!mainScene.startsWith("res://")) {
    // A non-res:// main scene is an outside reference; report it and never
    // construct or touch any filesystem path from it.
    return { exists: false, isSymlink: false, unsafe: true };
  }
  const relativePath = mainScene.slice("res://".length);
  if (relativePath.length === 0) {
    return { exists: false, isSymlink: false, unsafe: false };
  }
  // Lexical containment first: an outside reference is reported without any
  // filesystem call on it.
  const verdict = validateProjectRelativePath(relativePath, GODOT_LIMITS.maxResReferencePathBytes);
  if (!verdict.ok) {
    return { exists: false, isSymlink: false, unsafe: true };
  }
  const absolute = join(canonicalRoot, verdict.value);
  const verified = await verifyProjectPathContainment(canonicalRoot, absolute, fsOps);
  if (!verified.ok) {
    if (verified.reason === "missing") {
      return { exists: false, isSymlink: false, unsafe: false };
    }
    return { exists: false, isSymlink: false, unsafe: true };
  }
  try {
    const metadata = await fsOps.lstat(absolute);
    return { exists: metadata.isFile(), isSymlink: metadata.isSymbolicLink(), unsafe: false };
  } catch {
    return { exists: false, isSymlink: false, unsafe: false };
  }
}
