import { createHash } from "node:crypto";
import { lstat, opendir, stat } from "node:fs/promises";
import path from "node:path";
import {
  buildWorkspaceSummary,
  extractGDScriptStructure,
  referenceIdentityAnchor,
  type ReferenceAlias,
  type ReferenceId,
  type ReferenceListRequest,
  type ReferenceListResult,
  type ReferenceReadRequest,
  type ReferenceReadResult,
  type ReferenceRevision,
  type ReferenceSearchRequest,
  type ReferenceSearchResult,
  type ReferenceAccessPort,
} from "@siralos/core";
import { enumerateDirectoryBounded } from "../fs/directory-enumeration.js";
import { readFileBounded } from "../fs/file-read.js";
import { decodeUtf8, looksBinary, splitIntoLines } from "../tools/workspace/text.js";
import { describeFsError } from "../tools/workspace/workspace-path.js";
import { resolveReferencePath } from "./reference-path.js";
import type { ReferenceRoot, RootProvider } from "./reference-materializer.js";

/**
 * Reference access port over one materialized root (Stage 3 milestone 5).
 *
 * All paths go through `resolveReferencePath` (containment + symlink
 * escapes → `invalid_path`). List/read/search are bounded with the same
 * class of caps as the workspace tools: bounded directory enumeration,
 * capped file reads (1 MiB; larger → `unsupported`), UTF-8 text only
 * (NUL-byte probe → `unsupported`), and a fully bounded recursive search
 * (directory/entry/file/input/output/time/depth/match budgets with
 * explicit truncation reasons). `.gd` files support `structural` and
 * `summary` modes by reusing the core GDScript parser — never forked.
 *
 * Alias/revision are NEVER inferred here: `referenceInfo` is the
 * registry-owned identity context supplied by the composition root (the
 * registry is the SINGLE owner of reference identity; the adapter never
 * resolves or refreshes references itself, and never infers identity from
 * a model-supplied path). A null `referenceInfo` result (unknown
 * reference) fails closed as `unavailable`.
 *
 * Cancellation: the core request types carry no AbortSignal, so the tools
 * perform coarse cancellation checks around access calls; the search
 * deadline bounds the worst-case runtime of a single call.
 */

export const REFERENCE_ACCESS_LIMITS = {
  maxListEntries: 200,
  maxReadFileSizeBytes: 1024 * 1024,
  maxReadContentChars: 64_000,
  maxSummaryBytes: 4096,
  maxSearchFileSizeBytes: 512 * 1024,
  maxSearchFiles: 500,
  maxSearchMatches: 100,
  maxSearchLineLengthChars: 400,
  /** Independent global traversal bounds for reference.search. */
  maxSearchDirectories: 2_000,
  maxSearchEntries: 25_000,
  maxSearchFilesConsidered: 2_000,
  maxSearchInputBytes: 64 * 1024 * 1024,
  maxSearchOutputBytes: 200_000,
  maxSearchDurationMs: 10_000,
  /** Maximum directory depth for reference.search (root counts). */
  maxSearchDepth: 64,
} as const;

export type ReferenceAccessLimits = {
  readonly [Key in keyof typeof REFERENCE_ACCESS_LIMITS]: number;
};

export interface ReferenceInfo {
  readonly alias: ReferenceAlias;
  /** Registry-owned revision; null when the reference is not ready. */
  readonly revision: ReferenceRevision | null;
}

export type ReferenceInfoProvider = (referenceId: ReferenceId) => ReferenceInfo | null;

export interface ReferenceAccessOptions {
  readonly roots: RootProvider;
  readonly referenceInfo: ReferenceInfoProvider;
  readonly limits?: Partial<ReferenceAccessLimits>;
}

type AccessContext = {
  readonly ok: true;
  readonly alias: ReferenceAlias;
  readonly revision: ReferenceRevision;
  readonly root: ReferenceRoot;
};

async function resolveContext(
  referenceId: ReferenceId,
  options: ReferenceAccessOptions,
): Promise<AccessContext | { readonly ok: false; readonly reason: string }> {
  const info = options.referenceInfo(referenceId);
  if (info === null) {
    return { ok: false, reason: "Reference not configured." };
  }
  if (info.revision === null) {
    return { ok: false, reason: "Reference is not resolved; no revision is available." };
  }
  const root = await options.roots.rootFor(referenceId, info.revision.identity);
  if (root === null) {
    return { ok: false, reason: "Reference root is not accessible." };
  }
  return { ok: true, alias: info.alias, revision: info.revision, root };
}

function isNotFoundError(error: unknown): boolean {
  return (
    error instanceof Error &&
    "code" in error &&
    (error.code === "ENOENT" || error.code === "ENOTDIR")
  );
}

function childRelativePath(directoryPath: string, name: string): string {
  return directoryPath === "." ? name : `${directoryPath}/${name}`;
}

async function list(
  request: ReferenceListRequest,
  options: ReferenceAccessOptions,
  limits: ReferenceAccessLimits,
): Promise<ReferenceListResult> {
  const context = await resolveContext(request.reference, options);
  if (!context.ok) {
    return { status: "unavailable", reason: context.reason };
  }
  const resolved = await resolveReferencePath(context.root.path, request.path ?? ".");
  if (!resolved.ok) {
    return { status: "invalid_path", reason: resolved.reason };
  }
  let stats;
  try {
    stats = await stat(resolved.resolved);
  } catch (error: unknown) {
    return { status: "failed", reason: `Cannot inspect directory: ${describeFsError(error)}` };
  }
  if (!stats.isDirectory()) {
    return { status: "failed", reason: "Target is not a directory." };
  }
  // Entries are enumerated incrementally with a hard cap so a hostile
  // directory with millions of entries can never be materialized.
  const names: string[] = [];
  let truncated: boolean;
  try {
    const outcome = await enumerateDirectoryBounded({
      directory: resolved.resolved,
      maxEntries: limits.maxListEntries + 1,
      onEntry: (entry) => {
        names.push(entry.name);
      },
    });
    truncated = outcome.truncated;
  } catch (error: unknown) {
    return { status: "failed", reason: `Cannot list directory: ${describeFsError(error)}` };
  }
  names.sort();
  truncated = truncated || names.length > limits.maxListEntries;
  const selectedNames = names.slice(0, limits.maxListEntries);
  const entries: ReferenceEntryItem[] = [];
  for (const name of selectedNames) {
    let entryStats;
    try {
      entryStats = await lstat(path.join(resolved.resolved, name));
    } catch (error: unknown) {
      return { status: "failed", reason: `Cannot inspect entry: ${describeFsError(error)}` };
    }
    const entryPath = childRelativePath(resolved.relative, name);
    if (entryStats.isSymbolicLink()) {
      entries.push({ name, path: entryPath, type: "symlink" });
    } else if (entryStats.isDirectory()) {
      entries.push({ name, path: entryPath, type: "directory" });
    } else if (entryStats.isFile()) {
      entries.push({ name, path: entryPath, type: "file", size: entryStats.size });
    } else {
      entries.push({ name, path: entryPath, type: "other" });
    }
  }
  return {
    status: "ok",
    referenceId: request.reference,
    alias: context.alias,
    revision: context.revision,
    path: resolved.relative,
    entries,
    truncated,
  };
}

type ReferenceEntryItem = {
  readonly name: string;
  readonly path: string;
  readonly type: "file" | "directory" | "symlink" | "other";
  readonly size?: number;
};

async function read(
  request: ReferenceReadRequest,
  options: ReferenceAccessOptions,
  limits: ReferenceAccessLimits,
): Promise<ReferenceReadResult> {
  const context = await resolveContext(request.reference, options);
  if (!context.ok) {
    return { status: "unavailable", reason: context.reason };
  }
  const resolved = await resolveReferencePath(context.root.path, request.path);
  if (!resolved.ok) {
    return { status: "invalid_path", reason: resolved.reason };
  }
  let stats;
  try {
    stats = await stat(resolved.resolved);
  } catch (error: unknown) {
    if (isNotFoundError(error)) {
      return { status: "not_found", reason: "File does not exist." };
    }
    return { status: "failed", reason: `Cannot inspect file: ${describeFsError(error)}` };
  }
  if (!stats.isFile()) {
    return { status: "failed", reason: "Target is not a regular file." };
  }
  if (stats.size > limits.maxReadFileSizeBytes) {
    return {
      status: "unsupported",
      reason: `File is too large (${stats.size} bytes; limit ${limits.maxReadFileSizeBytes}).`,
    };
  }
  // The read itself is capped: a file grown or swapped after the stat is
  // read only up to the size bound plus one byte, so a hostile
  // replacement can never drive an unbounded read or block on a FIFO.
  const buffer = await readFileBounded(resolved.resolved, limits.maxReadFileSizeBytes);
  if (buffer === null) {
    return {
      status: "failed",
      reason: `Cannot read file: it is missing, not a regular file, or exceeds the ${limits.maxReadFileSizeBytes}-byte limit.`,
    };
  }
  if (looksBinary(buffer)) {
    return { status: "unsupported", reason: "File appears to be binary." };
  }
  const text = decodeUtf8(buffer);
  if (text === null) {
    return { status: "unsupported", reason: "File is not valid UTF-8 text." };
  }
  const sha256 = createHash("sha256").update(buffer).digest("hex");
  if (request.mode === "structural" || request.mode === "summary") {
    if (!resolved.relative.toLowerCase().endsWith(".gd")) {
      return {
        status: "unsupported",
        reason: "Structural and summary modes support GDScript (.gd) files only.",
      };
    }
    const structure = extractGDScriptStructure(text, resolved.relative);
    if (request.mode === "structural") {
      return {
        status: "ok",
        referenceId: request.reference,
        alias: context.alias,
        revision: context.revision,
        path: resolved.relative,
        sha256,
        content: null,
        structure,
        summary: null,
        truncated: false,
      };
    }
    // Bounded advisory summary; the revision slot states the reference
    // anchor (commit/fingerprint) so the summary always states its
    // revision, mirroring the workspace summary contract.
    const summary = buildWorkspaceSummary(structure, referenceIdentityAnchor(context.revision), {
      maxBytes: limits.maxSummaryBytes,
    });
    return {
      status: "ok",
      referenceId: request.reference,
      alias: context.alias,
      revision: context.revision,
      path: resolved.relative,
      sha256,
      content: null,
      structure: null,
      summary: summary.text,
      truncated: summary.truncated,
    };
  }
  const lines = splitIntoLines(text);
  const totalLines = lines.length;
  // Exact mode without slicing returns the raw text verbatim (bounded by
  // the content cap below); slicing is applied only when requested.
  if (request.startLine === undefined && request.endLine === undefined) {
    let raw = text;
    let rawTruncated = false;
    if (raw.length > limits.maxReadContentChars) {
      raw = raw.slice(0, limits.maxReadContentChars);
      rawTruncated = true;
    }
    return {
      status: "ok",
      referenceId: request.reference,
      alias: context.alias,
      revision: context.revision,
      path: resolved.relative,
      sha256,
      content: raw,
      structure: null,
      summary: null,
      truncated: rawTruncated,
    };
  }
  const startLine = request.startLine ?? 1;
  if (startLine > totalLines) {
    return {
      status: "failed",
      reason: `"startLine" (${startLine}) is beyond the end of the file (${totalLines} lines).`,
    };
  }
  const endLine = Math.min(request.endLine ?? totalLines, totalLines);
  let content = lines.slice(startLine - 1, endLine).join("\n");
  let truncated = false;
  if (content.length > limits.maxReadContentChars) {
    content = content.slice(0, limits.maxReadContentChars);
    truncated = true;
  }
  return {
    status: "ok",
    referenceId: request.reference,
    alias: context.alias,
    revision: context.revision,
    path: resolved.relative,
    sha256,
    content,
    structure: null,
    summary: null,
    truncated,
  };
}

type PendingDirectory = {
  readonly absolute: string;
  readonly relative: string;
  readonly depth: number;
};

type SearchMatch = {
  readonly path: string;
  readonly line: number;
  readonly column: number;
  readonly text: string;
};

type TruncationReason =
  | "directory_budget"
  | "entry_budget"
  | "file_budget"
  | "scan_budget"
  | "input_budget"
  | "output_budget"
  | "time_budget"
  | "match_limit"
  | "depth_budget";

type SearchOutcome =
  | { readonly status: "failed"; readonly reason: string }
  | {
      readonly status: "done";
      readonly matches: readonly SearchMatch[];
      readonly scannedFiles: number;
      readonly skippedFiles: number;
      readonly truncated: boolean;
      readonly truncationReason: string | null;
    };

function compareMatches(a: SearchMatch, b: SearchMatch): number {
  if (a.path !== b.path) {
    return a.path < b.path ? -1 : 1;
  }
  if (a.line !== b.line) {
    return a.line - b.line;
  }
  return a.column - b.column;
}

/**
 * Bounded recursive content search with the same class of independent
 * traversal budgets as workspace.search: no exclusions (references are
 * external material, not workspace policy), symlinks never traversed,
 * binary/non-UTF-8/oversized files skipped, explicit truncation reasons.
 */
async function boundedSearch(
  root: string,
  rootRelative: string,
  query: string,
  maxResults: number,
  limits: ReferenceAccessLimits,
): Promise<SearchOutcome> {
  const matches: SearchMatch[] = [];
  let scannedFiles = 0;
  let skippedFiles = 0;
  let directoriesVisited = 0;
  let entriesExamined = 0;
  let filesConsidered = 0;
  let inputBytes = 0;
  let outputBytes = 0;
  let truncated = false;
  let truncationReason: TruncationReason | null = null;
  const deadline = Date.now() + limits.maxSearchDurationMs;
  const stop = (reason: TruncationReason): SearchOutcome => {
    truncated = true;
    truncationReason = reason;
    return {
      status: "done",
      matches: [...matches].sort(compareMatches),
      scannedFiles,
      skippedFiles,
      truncated,
      truncationReason,
    };
  };
  const pendingDirectories: PendingDirectory[] = [
    { absolute: root, relative: rootRelative, depth: 0 },
  ];
  while (pendingDirectories.length > 0) {
    if (Date.now() >= deadline) {
      return stop("time_budget");
    }
    const directory = pendingDirectories.pop();
    if (directory === undefined) {
      break;
    }
    directoriesVisited += 1;
    if (directoriesVisited > limits.maxSearchDirectories) {
      return stop("directory_budget");
    }
    if (directory.depth > limits.maxSearchDepth) {
      return stop("depth_budget");
    }
    const names: string[] = [];
    let directoryHandle;
    try {
      directoryHandle = await opendir(directory.absolute);
    } catch {
      continue;
    }
    try {
      for await (const entry of directoryHandle) {
        entriesExamined += 1;
        if (entriesExamined > limits.maxSearchEntries) {
          return stop("entry_budget");
        }
        names.push(entry.name);
      }
    } finally {
      await directoryHandle.close().catch(() => undefined);
    }
    names.sort();
    for (const name of names) {
      if (Date.now() >= deadline) {
        return stop("time_budget");
      }
      const absolute = path.join(directory.absolute, name);
      let stats;
      try {
        stats = await lstat(absolute);
      } catch {
        skippedFiles += 1;
        continue;
      }
      if (stats.isSymbolicLink()) {
        skippedFiles += 1;
        continue;
      }
      if (stats.isDirectory()) {
        pendingDirectories.push({
          absolute,
          relative: childRelativePath(directory.relative, name),
          depth: directory.depth + 1,
        });
        continue;
      }
      if (!stats.isFile()) {
        skippedFiles += 1;
        continue;
      }
      filesConsidered += 1;
      if (filesConsidered > limits.maxSearchFilesConsidered) {
        return stop("file_budget");
      }
      if (stats.size > limits.maxSearchFileSizeBytes) {
        skippedFiles += 1;
        continue;
      }
      if (scannedFiles >= limits.maxSearchFiles) {
        return stop("scan_budget");
      }
      scannedFiles += 1;
      // The read itself is capped: a file grown or swapped after the
      // lstat is read only up to the size bound plus one byte.
      const buffer = await readFileBounded(absolute, limits.maxSearchFileSizeBytes);
      if (buffer === null) {
        skippedFiles += 1;
        continue;
      }
      inputBytes += buffer.length;
      if (inputBytes > limits.maxSearchInputBytes) {
        return stop("input_budget");
      }
      if (looksBinary(buffer)) {
        skippedFiles += 1;
        continue;
      }
      const text = decodeUtf8(buffer);
      if (text === null) {
        skippedFiles += 1;
        continue;
      }
      const relativePath = childRelativePath(directory.relative, name);
      const lines = splitIntoLines(text);
      for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
        const line = lines[lineIndex];
        if (line === undefined) {
          continue;
        }
        if ((lineIndex & 63) === 0 && Date.now() >= deadline) {
          return stop("time_budget");
        }
        const column = line.indexOf(query);
        if (column >= 0) {
          const matchText = line.slice(0, limits.maxSearchLineLengthChars);
          matches.push({
            path: relativePath,
            line: lineIndex + 1,
            column: column + 1,
            text: matchText,
          });
          outputBytes += Buffer.byteLength(matchText, "utf8");
          if (outputBytes > limits.maxSearchOutputBytes) {
            return stop("output_budget");
          }
          if (matches.length >= maxResults) {
            return stop("match_limit");
          }
        }
      }
    }
  }
  return {
    status: "done",
    matches: [...matches].sort(compareMatches),
    scannedFiles,
    skippedFiles,
    truncated,
    truncationReason,
  };
}

async function search(
  request: ReferenceSearchRequest,
  options: ReferenceAccessOptions,
  limits: ReferenceAccessLimits,
): Promise<ReferenceSearchResult> {
  const context = await resolveContext(request.reference, options);
  if (!context.ok) {
    return { status: "unavailable", reason: context.reason };
  }
  const resolved = await resolveReferencePath(context.root.path, request.path ?? ".");
  if (!resolved.ok) {
    return { status: "invalid_path", reason: resolved.reason };
  }
  const maxResults = Math.min(request.maxResults ?? 20, limits.maxSearchMatches);
  const outcome = await boundedSearch(
    resolved.resolved,
    resolved.relative,
    request.query,
    maxResults,
    limits,
  );
  if (outcome.status === "failed") {
    return { status: "failed", reason: outcome.reason };
  }
  return {
    status: "ok",
    referenceId: request.reference,
    alias: context.alias,
    revision: context.revision,
    query: request.query,
    matches: outcome.matches,
    scannedFiles: outcome.scannedFiles,
    skippedFiles: outcome.skippedFiles,
    truncated: outcome.truncated,
    truncationReason: outcome.truncationReason,
  };
}

export function createReferenceAccess(options: ReferenceAccessOptions): ReferenceAccessPort {
  const limits: ReferenceAccessLimits = { ...REFERENCE_ACCESS_LIMITS, ...options.limits };
  return {
    list(request: ReferenceListRequest): Promise<ReferenceListResult> {
      return list(request, options, limits);
    },
    read(request: ReferenceReadRequest): Promise<ReferenceReadResult> {
      return read(request, options, limits);
    },
    search(request: ReferenceSearchRequest): Promise<ReferenceSearchResult> {
      return search(request, options, limits);
    },
  };
}
