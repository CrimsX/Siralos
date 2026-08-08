import {
  parseDeclaredVersion,
  GODOT_LIMITS,
  type GodotApplicationEvent,
  type GodotProjectProfile,
  type SafeDiagnostic,
} from "@solaris/core";
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
 * before any filesystem access. The profile cache is keyed by the complete
 * `project.godot` SHA-256: each `inspect()` re-reads the bounded project
 * file and reuses the cached profile only when the file is unchanged, so
 * creating, editing, or deleting `project.godot` during a session is
 * visible on the next inspection.
 */
export function createGodotProjectInspector(
  dependencies: GodotProjectInspectorDependencies,
): GodotProjectInspector {
  const fsOps = dependencies.fsOps ?? DEFAULT_FS_OPS;
  let cached: { readonly profile: GodotProjectProfile; readonly sha256: string | null } | null =
    null;

  async function inspect(signal?: AbortSignal): Promise<GodotProjectProfile> {
    let canonicalRoot: string;
    try {
      canonicalRoot = await fsOps.realpath(dependencies.workspaceRoot);
    } catch {
      const profile = notDetected(
        {
          severity: "warning",
          message: "The workspace root could not be resolved; project inspection is unavailable.",
        },
        dependencies,
      );
      cached = { profile, sha256: null };
      return profile;
    }
    const read = await readProjectFile(canonicalRoot, signal, fsOps);
    if (cached !== null) {
      const cacheMatches =
        (read.ok && cached.sha256 === read.sha256) || (!read.ok && cached.sha256 === null);
      if (cacheMatches) {
        return cached.profile;
      }
    }
    if (!read.ok) {
      const profile = notDetected(
        {
          severity: "info",
          message:
            read.reason === "missing"
              ? "No project.godot exists at the workspace root; this is not a detected Godot project."
              : read.message,
        },
        dependencies,
      );
      cached = { profile, sha256: null };
      return profile;
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
    cached = { profile, sha256: read.sha256 };
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
