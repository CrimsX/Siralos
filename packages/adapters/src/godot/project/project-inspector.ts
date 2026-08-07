import {
  parseDeclaredVersion,
  type GodotApplicationEvent,
  type GodotProjectProfile,
  type SafeDiagnostic,
} from "@solaris/core";
import { scanProjectFile } from "./project-scanner.js";
import { readProjectFile } from "./project-files.js";
import { detectLanguageProfile } from "./language-profile.js";
import { inventoryExecutableContent } from "./content-inventory.js";
import { lstat } from "node:fs/promises";
import { join } from "node:path";

export interface GodotProjectInspectorDependencies {
  readonly workspaceRoot: string;
  readonly onEvent?: (event: GodotApplicationEvent) => void;
}

export interface GodotProjectInspector {
  inspect(signal?: AbortSignal): Promise<GodotProjectProfile>;
}

/**
 * Static project inspection: root-only `project.godot` detection, a
 * conservative scanner, language-profile evidence, and an executable-content
 * inventory. Nothing is executed, imported, or mutated; all results are
 * static and non-authoritative.
 */
export function createGodotProjectInspector(
  dependencies: GodotProjectInspectorDependencies,
): GodotProjectInspector {
  let cached: GodotProjectProfile | null = null;

  async function inspect(signal?: AbortSignal): Promise<GodotProjectProfile> {
    if (cached !== null) {
      return cached;
    }
    const read = await readProjectFile(dependencies.workspaceRoot, signal);
    if (!read.ok) {
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
        },
        warnings: [
          {
            severity: "info",
            message:
              read.reason === "missing"
                ? "No project.godot exists at the workspace root; this is not a detected Godot project."
                : read.message,
          },
        ],
      };
      dependencies.onEvent?.({ type: "godot_project_inspected", detected: false, warnings: 0 });
      cached = profile;
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
    const mainSceneInfo = await inspectMainScene(dependencies.workspaceRoot, mainScene);
    if (mainSceneInfo !== null) {
      if (mainSceneInfo.isSymlink) {
        warnings.push({
          severity: "warning",
          message: `The main scene ${mainScene} is a symbolic link.`,
        });
      }
    }
    const language = await detectLanguageProfile({
      workspaceRoot: dependencies.workspaceRoot,
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
      workspaceRoot: dependencies.workspaceRoot,
      ...(signal === undefined ? {} : { signal }),
      enabledPlugins: scan.enabledPlugins,
      autoloadCount: scan.autoloads.length,
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
    cached = profile;
    return profile;
  }

  return { inspect };
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
  workspaceRoot: string,
  mainScene: string | null,
): Promise<{ readonly exists: boolean; readonly isSymlink: boolean } | null> {
  if (mainScene === null || !mainScene.startsWith("res://")) {
    return null;
  }
  const relativePath = mainScene.slice("res://".length);
  if (relativePath.length === 0) {
    return { exists: false, isSymlink: false };
  }
  try {
    const metadata = await lstat(join(workspaceRoot, relativePath));
    return { exists: metadata.isFile(), isSymlink: metadata.isSymbolicLink() };
  } catch {
    return { exists: false, isSymlink: false };
  }
}
