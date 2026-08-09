import { createHash } from "node:crypto";
import { lstat, open } from "node:fs/promises";
import { dirname, join, sep } from "node:path";
import { samePathIdentity } from "../../fs-path-identity.js";
import {
  computeGodotRiskManifestDigest,
  GODOT_LIMITS,
  parseDeclaredVersion,
  type GodotEngineProfile,
  type GodotGDExtensionRiskEntry,
  type GodotInstallation,
  type GodotLibraryRiskEntry,
  type GodotPluginRiskEntry,
  type GodotProjectRiskManifest,
  type SafeDiagnostic,
} from "@solaris/core";
import { readProjectFile } from "../project/project-files.js";
import { scanProjectFile } from "../project/project-scanner.js";
import { inventoryExecutableContent } from "../project/content-inventory.js";
import { detectLanguageProfile } from "../project/language-profile.js";
import {
  DEFAULT_FS_OPS,
  validateProjectRelativePath,
  verifyProjectPathContainment,
} from "../project/traversal-limits.js";
import { scanAuthoredFiles } from "./authored-files.js";

/**
 * Shared static project-risk inventory for every project-aware Godot
 * operation (recovery probing and GDScript diagnostics).
 *
 * The manifest is the immutable digest binding for one-time approvals: it
 * records the exact project file, engine selection, tool scripts, enabled
 * editor plugins, GDExtension descriptors and their referenced libraries,
 * autoloads, .NET projects, the authored-file baseline, and warnings. All
 * reads are bounded, containment-verified, identity-checked, and
 * cancellation-aware. The source workspace is only ever read, never
 * written, by this inventory.
 */

export interface GodotRiskManifestRefreshRequest {
  readonly workspaceRoot: string;
  readonly installation: GodotInstallation;
  readonly profile: GodotEngineProfile;
  readonly signal?: AbortSignal;
}

export type GodotRiskManifestRefreshResult =
  | {
      readonly ok: true;
      readonly manifest: GodotProjectRiskManifest;
      readonly projectProfile: import("@solaris/core").GodotProjectProfile;
    }
  | { readonly ok: false; readonly message: string };

export async function refreshGodotProjectRiskManifest(
  request: GodotRiskManifestRefreshRequest,
): Promise<GodotRiskManifestRefreshResult> {
  const { workspaceRoot, installation, profile, signal } = request;
  const canonicalRoot = await canonicalWorkspaceRoot(workspaceRoot);
  if (canonicalRoot === null) {
    return { ok: false, message: "The workspace root could not be resolved." };
  }
  const read = await readProjectFile(canonicalRoot, signal);
  if (!read.ok) {
    return {
      ok: false,
      message:
        read.reason === "missing"
          ? "No project.godot exists at the workspace root; the project-aware operation cannot run."
          : read.message,
    };
  }
  const scan = scanProjectFile(read.content);
  const content = await inventoryExecutableContent({
    workspaceRoot: canonicalRoot,
    ...(signal === undefined ? {} : { signal }),
    enabledPlugins: scan.enabledPlugins,
    autoloadCount: scan.autoloads.length,
  });
  const language = await detectLanguageProfile({
    workspaceRoot: canonicalRoot,
    ...(signal === undefined ? {} : { signal }),
    dotnetAssemblyName: scan.dotnetAssemblyName,
    declaredFeatures: scan.declaredFeatures,
  });
  const authored = await scanAuthoredFiles({
    workspaceRoot: canonicalRoot,
    ...(signal === undefined ? {} : { signal }),
  });
  const toolScripts = [];
  for (const relativePath of content.inventory.toolScripts) {
    const hashed = await hashWorkspaceFile(workspaceRoot, relativePath, signal);
    if (hashed !== null) {
      toolScripts.push(hashed);
    }
  }
  const enabledEditorPlugins: GodotPluginRiskEntry[] = [];
  for (const plugin of content.inventory.editorPlugins) {
    if (!plugin.enabled) {
      continue;
    }
    const hashed = await hashWorkspaceFile(
      workspaceRoot,
      join(plugin.path, plugin.scriptPath.replace(/^res:\/\//, ""))
        .split(sep)
        .join("/"),
      signal,
    );
    if (hashed !== null) {
      enabledEditorPlugins.push({
        path: plugin.path,
        name: plugin.name,
        enabled: true,
        sha256: hashed.sha256,
        bytes: hashed.bytes,
      });
    }
  }
  const gdextensionDescriptors: GodotGDExtensionRiskEntry[] = [];
  for (const descriptor of content.inventory.gdextensionDescriptors) {
    const hashed = await hashWorkspaceFile(workspaceRoot, descriptor.path, signal);
    if (hashed === null) {
      continue;
    }
    const referencedLibraries: GodotLibraryRiskEntry[] = [];
    for (const target of descriptor.libraryTargets) {
      // Library targets are project-controlled: the absolute target must
      // pass lexical + canonical containment before any filesystem call.
      // An escaping target is reported as an unverified library (null
      // hash) and never read.
      const relativeTarget = target.startsWith("res://")
        ? target.slice("res://".length)
        : join(dirname(descriptor.path), target.replace(/^\.\//, "")).split(sep).join("/");
      let verifiedTarget: string | null = null;
      if (
        validateProjectRelativePath(relativeTarget, GODOT_LIMITS.maxResReferencePathBytes).ok ===
        true
      ) {
        const verified = await verifyProjectPathContainment(
          canonicalRoot,
          join(canonicalRoot, relativeTarget),
          DEFAULT_FS_OPS,
        );
        if (verified.ok) {
          verifiedTarget = verified.canonicalPath;
        }
      }
      const library =
        verifiedTarget === null ? null : await hashAbsoluteFile(verifiedTarget, signal);
      referencedLibraries.push(
        library === null
          ? { path: target, sha256: null, bytes: null }
          : { path: target, sha256: library.sha256, bytes: library.bytes },
      );
    }
    gdextensionDescriptors.push({
      path: descriptor.path,
      sha256: hashed.sha256,
      bytes: hashed.bytes,
      referencedLibraries,
    });
  }
  const warnings: SafeDiagnostic[] = [
    ...content.warnings,
    ...language.evidence.map((message) => ({ severity: "info" as const, message })),
  ];
  if (authored.truncated) {
    warnings.push({
      severity: "warning",
      message:
        "The authored-file baseline was truncated by its bounds; integrity coverage is bounded.",
    });
  }
  const manifestWithoutDigest: Omit<GodotProjectRiskManifest, "digest"> = {
    projectFileSha256: read.sha256,
    engineSelection: {
      installationId: installation.id,
      executableSha256: installation.sha256,
      version: profile.version.raw,
    },
    toolScripts,
    enabledEditorPlugins,
    gdextensionDescriptors,
    autoloads: scan.autoloads.map((autoload) => ({
      name: autoload.name,
      target: autoload.target,
    })),
    dotnetProjects: content.inventory.dotnetProjectFiles,
    authoredFileManifest: {
      fileCount: authored.fileCount,
      totalBytes: authored.totalBytes,
      digest: authored.digest,
      truncated: authored.truncated,
    },
    scanWarnings: warnings,
  };
  const manifest: GodotProjectRiskManifest = {
    ...manifestWithoutDigest,
    digest: computeGodotRiskManifestDigest(manifestWithoutDigest),
  };
  const declaredEngineVersion = extractDeclaredEngineVersion(scan.declaredFeatures);
  const projectProfile: import("@solaris/core").GodotProjectProfile = {
    detected: true,
    projectFileSha256: read.sha256,
    configVersion: scan.configVersion,
    name: scan.name,
    applicationVersion: scan.applicationVersion,
    declaredFeatures: scan.declaredFeatures,
    declaredEngineVersion,
    mainScene: scan.mainScene,
    mainSceneExists: null,
    mainSceneIsSymlink: false,
    renderingMethods: scan.renderingMethods,
    languageProfile: language.profile,
    autoloads: scan.autoloads,
    enabledEditorPlugins: scan.enabledPlugins,
    executableContent: content.inventory,
    warnings,
  };
  return { ok: true, manifest, projectProfile };
}

async function hashWorkspaceFile(
  workspaceRoot: string,
  relativePath: string,
  signal: AbortSignal | undefined,
): Promise<{ readonly path: string; readonly sha256: string; readonly bytes: number } | null> {
  if (
    validateProjectRelativePath(relativePath, GODOT_LIMITS.maxResReferencePathBytes).ok !== true
  ) {
    return null;
  }
  const canonicalRoot = await canonicalWorkspaceRoot(workspaceRoot);
  if (canonicalRoot === null) {
    return null;
  }
  const verified = await verifyProjectPathContainment(
    canonicalRoot,
    join(canonicalRoot, relativePath),
    DEFAULT_FS_OPS,
  );
  if (!verified.ok) {
    return null;
  }
  return hashAbsoluteFile(verified.canonicalPath, signal);
}

const canonicalRootCache = new Map<string, string | null>();

async function canonicalWorkspaceRoot(workspaceRoot: string): Promise<string | null> {
  const cached = canonicalRootCache.get(workspaceRoot);
  if (cached !== undefined) {
    return cached;
  }
  try {
    const { realpath } = await import("node:fs/promises");
    const canonical = await realpath(workspaceRoot);
    canonicalRootCache.set(workspaceRoot, canonical);
    return canonical;
  } catch {
    canonicalRootCache.set(workspaceRoot, null);
    return null;
  }
}

export async function hashAbsoluteFile(
  absolutePath: string,
  signal: AbortSignal | undefined,
): Promise<{ readonly path: string; readonly sha256: string; readonly bytes: number } | null> {
  // The path identity anchor is the containment-verified canonical path.
  // The leaf itself is lstat'd without following: a symlink/junction leaf
  // (even one planted inside the workspace) must never dereference to an
  // outside object, so its content is never read or hashed.
  let leafMetadata;
  try {
    leafMetadata = await lstat(absolutePath);
  } catch {
    return null;
  }
  if (leafMetadata.isSymbolicLink() || !leafMetadata.isFile()) {
    return null;
  }
  if (leafMetadata.size > GODOT_LIMITS.maxMirrorSingleFileBytes) {
    return null;
  }
  let handle;
  try {
    handle = await open(absolutePath, "r");
  } catch {
    return null;
  }
  try {
    const hash = createHash("sha256");
    const buffer = Buffer.alloc(1024 * 1024);
    for (;;) {
      if (signal?.aborted) {
        throw createAbortError();
      }
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, null);
      if (bytesRead === 0) {
        break;
      }
      hash.update(buffer.subarray(0, bytesRead));
    }
    // The path must still resolve to the same object after the read; a
    // swap during inspection discards the result rather than trusting it.
    const after = await realpathOf(absolutePath);
    if (after === null || !samePathIdentity(after, absolutePath)) {
      return null;
    }
    return {
      path: absolutePath.split(sep).join("/"),
      sha256: hash.digest("hex"),
      bytes: leafMetadata.size,
    };
  } catch (error: unknown) {
    if (isAbortError(error)) {
      throw error;
    }
    return null;
  } finally {
    await handle.close().catch(() => undefined);
  }
}

async function realpathOf(path: string): Promise<string | null> {
  try {
    const { realpath } = await import("node:fs/promises");
    return await realpath(path);
  } catch {
    return null;
  }
}

export function extractDeclaredEngineVersion(features: readonly string[]): {
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

export function isAbortError(error: unknown): boolean {
  return (
    error instanceof Error &&
    (error.name === "AbortError" || ("code" in error && error.code === "ABORT_ERR"))
  );
}

export function createAbortError(): Error {
  return new DOMException("The Godot project operation was aborted.", "AbortError");
}
