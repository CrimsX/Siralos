import { realpath } from "node:fs/promises";
import { join } from "node:path";
import { GODOT_LIMITS } from "@solaris/core";
import type { GodotEditionHint, GodotInstallation } from "@solaris/core";
import { godotCandidateNames, pathListSeparator } from "./candidate-names.js";
import { validateExecutable, type ExecutableIdentity } from "./executable-validation.js";

export interface PathDiscoveryOptions {
  readonly hostPath: string | null;
  readonly hostPathExt: string | null;
  readonly platform: NodeJS.Platform;
  readonly workspaceRoot: string;
  readonly signal?: AbortSignal;
}

/**
 * Conservative fixed-name PATH discovery. Only the sanitized host PATH is
 * searched for a fixed set of executable names; no disk, registry,
 * package-database, or Spotlight scanning occurs, and no shell (`where`,
 * `which`) is ever invoked. On Windows, `PATHEXT` is honored safely: only a
 * `.exe` extension is ever appended, and shell scripts (`.bat`, `.cmd`)
 * are never considered. Candidates are canonicalized and deduplicated,
 * bounded in count, and returned sorted deterministically.
 */
export async function discoverOnPath(options: PathDiscoveryOptions): Promise<{
  readonly candidates: readonly GodotInstallation[];
  readonly truncated: boolean;
}> {
  const entries = splitPath(options.hostPath, options.platform);
  const names = godotCandidateNames(options.platform);
  const pathExt = parsePathExt(options.hostPathExt, options.platform);
  const seen = new Set<string>();
  const candidates: GodotInstallation[] = [];
  let truncated = false;
  let index = 0;
  for (const directory of entries) {
    for (const name of names) {
      if (signalAborted(options.signal)) {
        throw createAbortError();
      }
      const variantNames = applyPathExt(name, pathExt, options.platform);
      for (const variant of variantNames) {
        const candidatePath = join(directory, variant);
        let canonical: string;
        try {
          canonical = await realpath(candidatePath);
        } catch {
          continue;
        }
        const folded = foldForDedupe(canonical, options.platform);
        if (seen.has(folded)) {
          continue;
        }
        if (candidates.length >= GODOT_LIMITS.maxCandidates) {
          truncated = true;
          break;
        }
        seen.add(folded);
        const validated = await validateExecutable({
          path: candidatePath,
          workspaceRoot: options.workspaceRoot,
          ...(options.signal === undefined ? {} : { signal: options.signal }),
        });
        const id = `path-${index + 1}`;
        index += 1;
        if (validated.ok) {
          candidates.push(
            installationFromIdentity(id, "path", "PATH", validated.identity, "unknown"),
          );
        } else {
          candidates.push(invalidInstallation(id, "path", "PATH", validated.error));
        }
      }
    }
  }
  candidates.sort((left, right) => left.id.localeCompare(right.id));
  return { candidates, truncated };
}

export function installationFromIdentity(
  id: string,
  source: "user-config" | "path",
  sourceLabel: string,
  identity: ExecutableIdentity,
  editionHint: GodotEditionHint,
): GodotInstallation {
  return {
    id,
    sourceLabel,
    source,
    canonicalPath: identity.canonicalPath,
    sizeBytes: identity.sizeBytes,
    modifiedAtMs: identity.modifiedAtMs,
    sha256: identity.sha256,
    editionHint,
    status: "valid",
  };
}

export function invalidInstallation(
  id: string,
  source: "user-config" | "path",
  sourceLabel: string,
  error: string,
): GodotInstallation {
  return {
    id,
    sourceLabel,
    source,
    canonicalPath: "",
    sizeBytes: 0,
    modifiedAtMs: 0,
    sha256: "",
    editionHint: "unknown",
    status: "invalid",
    error,
  };
}

function splitPath(hostPath: string | null, platform: NodeJS.Platform): readonly string[] {
  if (hostPath === null || hostPath.trim().length === 0) {
    return [];
  }
  const separator = pathListSeparator(platform);
  const entries = hostPath.split(separator).filter((entry) => entry.trim().length > 0);
  return [...new Set(entries)];
}

function parsePathExt(hostPathExt: string | null, platform: NodeJS.Platform): readonly string[] {
  if (platform !== "win32" || hostPathExt === null) {
    return [];
  }
  const extensions = hostPathExt
    .split(";")
    .map((extension) => extension.trim().toLowerCase())
    .filter((extension) => extension.startsWith("."));
  return [...new Set(extensions)];
}

function applyPathExt(
  name: string,
  pathExt: readonly string[],
  platform: NodeJS.Platform,
): readonly string[] {
  if (platform !== "win32" || pathExt.length === 0) {
    return [name];
  }
  const hasExtension = /\.\w+$/.test(name);
  if (hasExtension) {
    // Only real binaries are considered; .bat/.cmd shell scripts never are.
    return name.toLowerCase().endsWith(".exe") ? [name] : [];
  }
  const variants = [name];
  for (const extension of pathExt) {
    if (extension === ".exe") {
      variants.push(`${name}${extension}`);
    }
  }
  return [...new Set(variants)];
}

function foldForDedupe(path: string, platform: NodeJS.Platform): string {
  return platform === "win32" || platform === "darwin" ? path.toLowerCase() : path;
}

function signalAborted(signal: AbortSignal | undefined): boolean {
  return signal?.aborted === true;
}

function createAbortError(): Error {
  return new DOMException("Godot PATH discovery was aborted.", "AbortError");
}
