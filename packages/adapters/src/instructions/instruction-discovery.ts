import { createHash } from "node:crypto";
import { lstat, opendir, realpath } from "node:fs/promises";
import path from "node:path";
import {
  MAX_INSTRUCTION_CONTENT_BYTES,
  buildInstruction,
  computeInstructionInventoryRevision,
  resolveInstructionSet,
  resolveInstructionsForPath,
  type InstructionDiscoveryOutcome,
  type ProjectInstruction,
  type ProjectInstructionService,
  type ResolvedInstructionSet,
  type WorkspaceRevisionRegistry,
} from "@siralos/core";
import { foldPathComponent } from "../fs-case.js";
import { readFileBounded } from "../fs/file-read.js";
import { decodeUtf8 } from "../tools/workspace/text.js";
import { resolveWorkspaceRoot } from "../tools/workspace/workspace-path.js";

/**
 * Project-instruction discovery (Stage 3 milestone 4 §8–§10).
 *
 * Discovers the repository's guidance convention (`AGENTS.md`) inside the
 * workspace with the same canonical containment as ordinary workspace
 * reads: the walk starts from the canonicalized root, never follows
 * symbolic links (a link is skipped, never traversed, so it cannot become
 * a filesystem escape), skips excluded directories, and is bounded in
 * depth, entries, files, and bytes. Discovery is read-only: it never
 * writes, never fetches network URLs, and treats any URL inside an
 * instruction file as plain text.
 *
 * Every discovered instruction is bound to the exact file revision
 * (SHA-256 through the session revision registry), so a changed file
 * changes the resolved instruction revision.
 */

export const INSTRUCTION_DISCOVERY_LIMITS = {
  maxInstructionFiles: 64,
  maxDepth: 16,
  maxEntriesPerDirectory: 4_096,
  maxDirectories: 512,
} as const;

export interface ProjectInstructionDiscoveryOptions {
  readonly workspaceRoot: string;
  readonly revisions?: WorkspaceRevisionRegistry;
  readonly platform?: NodeJS.Platform;
  readonly limits?: Partial<typeof INSTRUCTION_DISCOVERY_LIMITS>;
}

export async function discoverProjectInstructions(
  options: ProjectInstructionDiscoveryOptions,
): Promise<InstructionDiscoveryOutcome> {
  const root = await resolveWorkspaceRoot(options.workspaceRoot);
  const platform = options.platform ?? process.platform;
  const limits = { ...INSTRUCTION_DISCOVERY_LIMITS, ...options.limits };
  const excluded = new Set(
    ["node_modules", ".git", "dist", "coverage", ".siralos"].map((name) =>
      foldPathComponent(name, platform),
    ),
  );
  const instructions: ProjectInstruction[] = [];
  let truncated = false;
  let scannedDirectories = 0;
  let scannedFiles = 0;

  const pending: Array<{
    readonly absolute: string;
    readonly relative: string;
    readonly depth: number;
  }> = [{ absolute: root, relative: ".", depth: 0 }];
  while (pending.length > 0 && !truncated) {
    const directory = pending.shift() as (typeof pending)[number];
    scannedDirectories += 1;
    if (scannedDirectories > limits.maxDirectories) {
      truncated = true;
      break;
    }
    let handle;
    try {
      handle = await opendir(directory.absolute);
    } catch {
      continue; // an unreadable directory is skipped, never fatal
    }
    const names: string[] = [];
    try {
      let examined = 0;
      for await (const entry of handle) {
        examined += 1;
        if (examined > limits.maxEntriesPerDirectory) {
          truncated = true;
          break;
        }
        if (excluded.has(foldPathComponent(entry.name, platform))) {
          continue;
        }
        names.push(entry.name);
      }
    } finally {
      await handle.close().catch(() => undefined);
    }
    if (truncated) {
      break;
    }
    names.sort();
    for (const name of names) {
      const absolute = path.join(directory.absolute, name);
      let stats;
      try {
        stats = await lstat(absolute);
      } catch {
        continue;
      }
      if (stats.isSymbolicLink()) {
        continue; // never traverse links: containment is preserved
      }
      if (stats.isDirectory()) {
        if (directory.depth + 1 <= limits.maxDepth) {
          pending.push({
            absolute,
            relative: childRelativePath(directory.relative, name),
            depth: directory.depth + 1,
          });
        }
        continue;
      }
      if (!stats.isFile()) {
        continue;
      }
      if (foldPathComponent(name, platform) !== foldPathComponent("AGENTS.md", platform)) {
        continue;
      }
      if (instructions.length >= limits.maxInstructionFiles) {
        truncated = true;
        break;
      }
      const buffer = await readFileBounded(absolute, MAX_INSTRUCTION_CONTENT_BYTES);
      if (buffer === null) {
        continue; // oversized/unreadable instruction files are skipped
      }
      const text = decodeUtf8(buffer);
      if (text === null) {
        continue;
      }
      const relativePath = childRelativePath(directory.relative, name);
      const sha256 = sha256Of(buffer);
      const sourceRevision = options.revisions?.issue(relativePath, sha256) ?? null;
      const source =
        directory.relative === "."
          ? { kind: "project_root" as const, path: null }
          : { kind: "project_directory" as const, path: directory.relative };
      instructions.push(
        buildInstruction({
          source,
          content: text,
          scopePath: directory.relative === "." ? "." : directory.relative,
          sourceRevision,
        }),
      );
      scannedFiles += 1;
    }
  }
  return {
    instructions: [...instructions].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)),
    truncated,
    scannedDirectories,
    scannedFiles,
  };
}

function childRelativePath(parent: string, name: string): string {
  return parent === "." ? name : `${parent}/${name}`;
}

function sha256Of(buffer: Buffer): string {
  return createHash("sha256").update(buffer).digest("hex");
}

export function createProjectInstructionService(
  options: ProjectInstructionDiscoveryOptions,
): ProjectInstructionService {
  let inventory: readonly ProjectInstruction[] = [];
  let inventoryRevision: string | null = null;

  return {
    async load(): Promise<InstructionDiscoveryOutcome> {
      const outcome = await discoverProjectInstructions(options);
      inventory = outcome.instructions;
      inventoryRevision =
        outcome.instructions.length === 0
          ? null
          : computeInstructionInventoryRevision(outcome.instructions);
      return outcome;
    },

    async refresh(): Promise<InstructionDiscoveryOutcome> {
      return this.load();
    },

    instructions(): readonly ProjectInstruction[] {
      return [...inventory];
    },

    async resolveForPath(workspaceRelativePath: string): Promise<ResolvedInstructionSet> {
      const validated = await validateFocusPath(options.workspaceRoot, workspaceRelativePath);
      if (validated === null) {
        // An out-of-containment path cannot claim any scoped guidance.
        return resolveInstructionSet({ instructions: [], paths: [] });
      }
      return resolveInstructionsForPath(inventory, validated);
    },

    async resolveForPaths(paths: readonly string[]): Promise<ResolvedInstructionSet> {
      if (paths.length === 0) {
        return resolveInstructionsForPath(inventory, ".");
      }
      const validated: string[] = [];
      for (const path of paths) {
        const resolved = await validateFocusPath(options.workspaceRoot, path);
        if (resolved !== null) {
          validated.push(resolved);
        }
      }
      return resolveInstructionSet({ instructions: inventory, paths: validated });
    },

    revision(): string | null {
      return inventoryRevision;
    },
  };
}

async function validateFocusPath(workspaceRoot: string, requested: string): Promise<string | null> {
  // Syntactic rejection first: focus paths are workspace-relative.
  if (
    requested.length === 0 ||
    requested.includes("\0") ||
    requested.includes("\\") ||
    requested.startsWith("/") ||
    /^[A-Za-z]:/.test(requested)
  ) {
    return null;
  }
  const components = requested.split("/").filter((component) => component.length > 0);
  if (components.some((component) => component === ".." || component === ".")) {
    return null;
  }
  // Containment: canonicalize the nearest existing ancestor and require
  // it to stay inside the canonical root. The target itself need not
  // exist (a task may focus on a file it will create), but a symlinked
  // ancestor can never smuggle the scope outside the workspace.
  const canonicalRoot = await realpath(workspaceRoot).catch(() => null);
  if (canonicalRoot === null) {
    return null;
  }
  const prefix = canonicalRoot.endsWith(path.sep) ? canonicalRoot : `${canonicalRoot}${path.sep}`;
  let remaining = [...components];
  while (remaining.length > 0) {
    const candidate = path.join(canonicalRoot, ...remaining);
    const resolved = await realpath(candidate).catch(() => null);
    if (resolved !== null) {
      if (resolved !== canonicalRoot && !resolved.startsWith(prefix)) {
        return null;
      }
      break;
    }
    remaining = remaining.slice(0, -1);
  }
  return components.join("/");
}
