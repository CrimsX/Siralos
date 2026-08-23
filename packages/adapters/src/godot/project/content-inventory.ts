import { basename, dirname, join, relative, sep } from "node:path";
import type { Dirent } from "node:fs";
import {
  GODOT_LIMITS,
  type GodotExecutableContentInventory,
  type GodotGDExtensionSummary,
  type GodotPluginSummary,
  type GodotScanTruncationReason,
  type SafeDiagnostic,
} from "@siralos/core";
import { containsCodeToken } from "./lexical.js";
import { scanProjectFiles, type BoundedScanResult } from "./bounded-scan.js";
import {
  createTraversalBudget,
  DEFAULT_FS_OPS,
  readBoundedProjectFile,
  validateProjectRelativePath,
  verifyProjectPathContainment,
  type GodotProjectFsOps,
  type TraversalBudget,
} from "./traversal-limits.js";
import { samePathIdentity } from "../../fs-path-identity.js";
import { enumerateDirectoryBounded } from "../../fs/directory-enumeration.js";

export interface ContentInventoryOptions {
  readonly workspaceRoot: string;
  readonly signal?: AbortSignal;
  readonly enabledPlugins: readonly string[];
  readonly autoloadCount: number;
  /** Optional pre-computed traversal; deterministic tests inject a stub. */
  readonly scan?: Promise<BoundedScanResult>;
  /** Read seam; tests inject a stub to prove no outside read ever occurs. */
  readonly readHead?: (
    path: string,
    maxBytes: number,
  ) => Promise<
    | { readonly ok: true; readonly content: string; readonly bytesRead: number }
    | { readonly ok: false }
  >;
  /** Filesystem seam (lstat/realpath/readdir/open); tests spy on every call. */
  readonly fsOps?: GodotProjectFsOps;
  /** Injectable bounds for deterministic tests; defaults come from GODOT_LIMITS. */
  readonly timeoutMs?: number;
  readonly maxDirectories?: number;
  readonly maxEntries?: number;
  readonly maxFiles?: number;
  readonly maxSurfaced?: number;
  readonly maxPluginDirectories?: number;
  readonly maxDescriptorsParsed?: number;
  readonly maxInventoryItems?: number;
  readonly maxTotalReadBytes?: number;
  readonly maxSourceBytesInspected?: number;
  readonly maxDepth?: number;
}

type ReadHeadResult =
  | { readonly ok: true; readonly content: string; readonly bytesRead: number }
  | { readonly ok: false };

export type GodotProjectReadHead = (path: string, maxBytes: number) => Promise<ReadHeadResult>;

/**
 * Static executable-content inventory. Nothing is loaded or run: `@tool`
 * markers are detected lexically near the start of bounded `.gd` files,
 * `plugin.cfg` descriptors are parsed conservatively, import plugins are a
 * heuristic based on `extends EditorImportPlugin`, GDExtension descriptors
 * are read without loading native libraries, and all traversal is bounded
 * and symlink-safe. Every project-controlled path value is lexically and
 * canonically contained before any filesystem access; an outside reference
 * is reported as a warning using only its project-provided text.
 */
export async function inventoryExecutableContent(options: ContentInventoryOptions): Promise<{
  readonly inventory: GodotExecutableContentInventory;
  readonly warnings: readonly SafeDiagnostic[];
}> {
  const warnings: SafeDiagnostic[] = [];
  const fsOps = options.fsOps ?? DEFAULT_FS_OPS;
  const canonicalRoot = await fsOps.realpath(options.workspaceRoot).catch(() => null);
  if (canonicalRoot === null) {
    return {
      inventory: {
        toolScripts: [],
        editorPlugins: [],
        importPlugins: [],
        gdextensionDescriptors: [],
        autoloadCount: options.autoloadCount,
        dotnetProjectFiles: [],
        scanTruncated: false,
        scanTruncationReason: "none",
      },
      warnings: [
        {
          severity: "warning",
          message:
            "The workspace root could not be resolved; the executable-content inventory is empty.",
        },
      ],
    };
  }
  const budget = createTraversalBudget({
    timeoutMs: options.timeoutMs ?? GODOT_LIMITS.staticProjectScanTimeoutMs,
    maxFiles: options.maxFiles ?? GODOT_LIMITS.maxProjectFilesScanned,
    maxDirectories: options.maxDirectories ?? GODOT_LIMITS.maxProjectDirectoriesVisited,
    maxEntries: options.maxEntries ?? GODOT_LIMITS.maxProjectEntriesExamined,
    maxSurfaced: options.maxSurfaced ?? GODOT_LIMITS.maxProjectFilesSurfaced,
    maxReadBytes: options.maxTotalReadBytes ?? GODOT_LIMITS.maxProjectTotalReadBytes,
    maxPluginDirectories: options.maxPluginDirectories ?? GODOT_LIMITS.maxProjectPluginDirectories,
    maxDescriptorsParsed: options.maxDescriptorsParsed ?? GODOT_LIMITS.maxProjectDescriptorsParsed,
    maxInventoryItems: options.maxInventoryItems ?? GODOT_LIMITS.maxProjectInventoryItems,
    maxDepth: options.maxDepth ?? GODOT_LIMITS.maxProjectScanDepth,
  });
  const scan =
    options.scan ??
    scanProjectFiles({
      workspaceRoot: canonicalRoot,
      ...(options.signal === undefined ? {} : { signal: options.signal }),
      budget,
      onlyGdFiles: false,
      includeDotnet: true,
      includeGDExtensions: true,
    });
  const result = await scan;
  warnings.push(...result.warnings);
  const maxSourceBytes = options.maxSourceBytesInspected ?? GODOT_LIMITS.maxSourceBytesInspected;
  const readHead: GodotProjectReadHead = options.readHead ?? defaultReadHead(fsOps);
  const toolScripts: string[] = [];
  const dotnetProjectFiles: string[] = [];
  const gdextensionFiles: string[] = [];
  let sourceBytesInspected = 0;
  for (const file of result.files) {
    if (budget.exhausted) {
      break;
    }
    budget.checkCancelled(options.signal);
    if (!budget.isWithinDeadline()) {
      break;
    }
    const name = basename(file).toLowerCase();
    if (name.endsWith(".gd")) {
      if (sourceBytesInspected >= maxSourceBytes) {
        budget.stop("bytes-limit");
        warnings.push({
          severity: "warning",
          message:
            "The source inspection byte budget (maxSourceBytesInspected) was exhausted; the tool-script inventory is partial.",
        });
        break;
      }
      if (budget.bytesRead >= budget.maxReadBytes) {
        budget.stop("bytes-limit");
        warnings.push({
          severity: "warning",
          message:
            "The total read byte budget (maxProjectTotalReadBytes) was exhausted; the tool-script inventory is partial.",
        });
        break;
      }
      const verified = await verifyProjectPathContainment(canonicalRoot, file, fsOps);
      if (!verified.ok) {
        if (verified.reason === "outside" || verified.reason === "symlink") {
          warnings.push({
            severity: "warning",
            message: `The tool script ${workspaceRelative(canonicalRoot, file)} could not be safely resolved inside the workspace and was skipped.`,
          });
        }
        continue;
      }
      const head = await readHead(file, GODOT_LIMITS.maxToolScriptHeadBytes);
      if (!head.ok) {
        continue;
      }
      if (!budget.consumeBytes(head.bytesRead)) {
        warnings.push({
          severity: "warning",
          message:
            "The total read byte budget (maxProjectTotalReadBytes) was exhausted; the tool-script inventory is partial.",
        });
        break;
      }
      sourceBytesInspected += head.bytesRead;
      if (sourceBytesInspected > maxSourceBytes) {
        budget.stop("bytes-limit");
        warnings.push({
          severity: "warning",
          message:
            "The source inspection byte budget (maxSourceBytesInspected) was exhausted; the tool-script inventory is partial.",
        });
        break;
      }
      // Re-verify identity after the read: a parent or leaf swapped during
      // inspection is treated as an escape and the data is discarded.
      const after = await fsOps.realpath(file).catch(() => null);
      if (after === null || !samePathIdentity(after, verified.canonicalPath)) {
        warnings.push({
          severity: "warning",
          message: `The tool script ${workspaceRelative(canonicalRoot, file)} changed during inspection and was skipped.`,
        });
        continue;
      }
      if (containsCodeToken(head.content, "@tool")) {
        if (!budget.addInventoryItem()) {
          warnings.push({
            severity: "warning",
            message:
              "The executable-content inventory exceeded its item bound (maxProjectInventoryItems); the inventory is partial.",
          });
          break;
        }
        toolScripts.push(workspaceRelative(canonicalRoot, file));
      }
    } else if (name.endsWith(".csproj") || name.endsWith(".sln")) {
      if (!budget.addInventoryItem()) {
        warnings.push({
          severity: "warning",
          message:
            "The executable-content inventory exceeded its item bound (maxProjectInventoryItems); the inventory is partial.",
        });
        break;
      }
      dotnetProjectFiles.push(workspaceRelative(canonicalRoot, file));
    } else if (name.endsWith(".gdextension")) {
      gdextensionFiles.push(file);
    }
  }
  const editorPlugins = await scanEditorPlugins(
    options,
    canonicalRoot,
    budget,
    warnings,
    readHead,
    fsOps,
  );
  const gdextensionDescriptors = await scanGDExtensions(
    options,
    canonicalRoot,
    budget,
    gdextensionFiles,
    warnings,
    fsOps,
  );
  for (let index = 0; index < options.autoloadCount; index += 1) {
    if (!budget.addInventoryItem()) {
      warnings.push({
        severity: "warning",
        message:
          "The executable-content inventory exceeded its item bound (maxProjectInventoryItems); the inventory is partial.",
      });
      break;
    }
  }
  const scanTruncationReason: GodotScanTruncationReason =
    budget.reason !== "none" ? budget.reason : result.truncationReason;
  const inventory: GodotExecutableContentInventory = {
    toolScripts,
    editorPlugins,
    importPlugins: editorPlugins
      .filter((plugin) => plugin.importPluginHeuristic)
      .map((plugin) => plugin.path),
    gdextensionDescriptors,
    autoloadCount: options.autoloadCount,
    dotnetProjectFiles,
    scanTruncated: scanTruncationReason !== "none",
    scanTruncationReason,
  };
  return { inventory, warnings };
}

async function scanEditorPlugins(
  options: ContentInventoryOptions,
  canonicalRoot: string,
  budget: TraversalBudget,
  warnings: SafeDiagnostic[],
  readHead: GodotProjectReadHead,
  fsOps: GodotProjectFsOps,
): Promise<readonly GodotPluginSummary[]> {
  const addonsDirectory = join(canonicalRoot, "addons");
  // Entries are enumerated incrementally and collected only up to the
  // remaining entry budget, so a hostile addons directory can never
  // materialize an unbounded listing; the collected set is sorted for
  // deterministic output order.
  const remainingEntries = Math.max(0, budget.maxEntries - budget.entriesExamined);
  const collected: Dirent[] = [];
  let truncatedListing: boolean;
  try {
    const outcome = await enumerateDirectoryBounded({
      directory: addonsDirectory,
      maxEntries: remainingEntries,
      signal: options.signal,
      deadline: budget.deadline,
      onEntry: (entry) => {
        collected.push(entry);
      },
    });
    truncatedListing = outcome.truncated;
  } catch {
    return [];
  }
  const sorted = collected.sort((left, right) => left.name.localeCompare(right.name));
  const plugins: GodotPluginSummary[] = [];
  for (const entry of sorted) {
    if (budget.exhausted) {
      break;
    }
    budget.checkCancelled(options.signal);
    if (!budget.isWithinDeadline()) {
      break;
    }
    budget.entriesExamined += 1;
    if (budget.entriesExamined > budget.maxEntries) {
      budget.stop("entry-limit");
      warnings.push({
        severity: "warning",
        message:
          "The plugin enumeration stopped at the entries-examined bound (maxProjectEntriesExamined); the plugin inventory is partial.",
      });
      break;
    }
    if (entry.isSymbolicLink()) {
      warnings.push({
        severity: "warning",
        message: `The addon entry addons/${entry.name} is a symbolic link and was skipped.`,
      });
      continue;
    }
    if (!entry.isDirectory() || entry.name.startsWith(".")) {
      continue;
    }
    budget.pluginDirectories += 1;
    if (budget.pluginDirectories > budget.maxPluginDirectories) {
      budget.stop("plugin-limit");
      warnings.push({
        severity: "warning",
        message:
          "The number of editor plugin directories exceeded the bound (maxProjectPluginDirectories); the plugin inventory is partial.",
      });
      break;
    }
    const pluginDirectory = join(canonicalRoot, "addons", entry.name);
    const pluginFile = join(pluginDirectory, "plugin.cfg");
    budget.descriptorsParsed += 1;
    if (budget.descriptorsParsed > budget.maxDescriptorsParsed) {
      budget.stop("descriptor-limit");
      warnings.push({
        severity: "warning",
        message:
          "The number of parsed project descriptors exceeded the bound (maxProjectDescriptorsParsed); the plugin inventory is partial.",
      });
      break;
    }
    if (budget.bytesRead >= budget.maxReadBytes) {
      budget.stop("bytes-limit");
      warnings.push({
        severity: "warning",
        message:
          "The total read byte budget (maxProjectTotalReadBytes) was exhausted; the plugin inventory is partial.",
      });
      break;
    }
    const read = await readBoundedProjectFile({
      canonicalRoot,
      path: pluginFile,
      maxBytes: GODOT_LIMITS.maxPluginDescriptorBytes,
      fsOps,
    });
    if (!read.ok) {
      if (read.reason === "oversized") {
        warnings.push({
          severity: "warning",
          message: `The editor plugin descriptor addons/${entry.name}/plugin.cfg exceeds the size limit and was skipped.`,
        });
      } else if (read.reason === "symlink") {
        warnings.push({
          severity: "warning",
          message: `The editor plugin descriptor addons/${entry.name}/plugin.cfg is not a regular file and was skipped.`,
        });
      } else if (read.reason === "outside" || read.reason === "changed") {
        warnings.push({
          severity: "warning",
          message: `The editor plugin descriptor addons/${entry.name}/plugin.cfg could not be safely resolved and was skipped.`,
        });
      }
      continue;
    }
    if (!budget.consumeBytes(read.bytesRead)) {
      warnings.push({
        severity: "warning",
        message:
          "The total read byte budget (maxProjectTotalReadBytes) was exhausted; the plugin inventory is partial.",
      });
      break;
    }
    const parsed = parsePluginDescriptor(read.content);
    if (!pluginDescriptorWithinBounds(parsed)) {
      warnings.push({
        severity: "warning",
        message: `The editor plugin descriptor addons/${entry.name}/plugin.cfg contains a value exceeding the descriptor value bound (maxProjectDescriptorValueBytes) and was skipped.`,
      });
      continue;
    }
    const pluginPath = `addons/${entry.name}`;
    const enabled = options.enabledPlugins.includes(`res://${pluginPath}`);
    const scriptValue = parsed.script;
    const language = scriptValue.endsWith(".cs")
      ? ("dotnet" as const)
      : scriptValue.endsWith(".gd")
        ? ("gdscript" as const)
        : ("unknown" as const);
    let importPluginHeuristic = false;
    if (language === "gdscript" && scriptValue.length > 0) {
      // The script value is attacker-controlled: reject unsafe spellings
      // lexically before constructing any path, then verify canonical
      // containment before the read and re-verify identity after it.
      const scriptReference = scriptValue.startsWith("res://")
        ? scriptValue.slice("res://".length)
        : scriptValue;
      const verdict = validateProjectRelativePath(
        scriptReference,
        GODOT_LIMITS.maxResReferencePathBytes,
      );
      if (!verdict.ok) {
        warnings.push({
          severity: "warning",
          message: `The editor plugin ${pluginPath} declares a script (${scriptValue}) that is not a contained project path and it was not inspected.`,
        });
      } else {
        const scriptBase = scriptValue.startsWith("res://") ? canonicalRoot : pluginDirectory;
        const scriptAbsolute = join(scriptBase, verdict.value);
        const verified = await verifyProjectPathContainment(canonicalRoot, scriptAbsolute, fsOps);
        if (!verified.ok) {
          if (verified.reason === "outside" || verified.reason === "symlink") {
            warnings.push({
              severity: "warning",
              message: `The editor plugin ${pluginPath} script (${scriptValue}) could not be safely resolved inside the workspace and it was not inspected.`,
            });
          }
        } else {
          const head = await readHead(scriptAbsolute, GODOT_LIMITS.maxToolScriptHeadBytes);
          if (head.ok) {
            if (!budget.consumeBytes(head.bytesRead)) {
              warnings.push({
                severity: "warning",
                message:
                  "The total read byte budget (maxProjectTotalReadBytes) was exhausted; the plugin inventory is partial.",
              });
              break;
            }
            const after = await fsOps.realpath(scriptAbsolute).catch(() => null);
            if (after === null || !samePathIdentity(after, verified.canonicalPath)) {
              warnings.push({
                severity: "warning",
                message: `The editor plugin ${pluginPath} script (${scriptValue}) changed during inspection and it was not inspected.`,
              });
            } else if (containsCodeToken(head.content, "extends EditorImportPlugin")) {
              importPluginHeuristic = true;
            }
          }
        }
      }
    }
    if (!budget.addInventoryItem()) {
      warnings.push({
        severity: "warning",
        message:
          "The executable-content inventory exceeded its item bound (maxProjectInventoryItems); the plugin inventory is partial.",
      });
      break;
    }
    plugins.push({
      path: pluginPath,
      name: parsed.name,
      description: parsed.description,
      author: parsed.author,
      version: parsed.version,
      scriptPath: scriptValue,
      language,
      enabled,
      importPluginHeuristic,
    });
  }
  if (truncatedListing && !budget.exhausted) {
    budget.stop("entry-limit");
    warnings.push({
      severity: "warning",
      message:
        "The plugin enumeration stopped at the entries-examined bound (maxProjectEntriesExamined); the plugin inventory is partial.",
    });
  }
  return plugins;
}

interface PluginDescriptor {
  readonly name: string;
  readonly description: string;
  readonly author: string;
  readonly version: string;
  readonly script: string;
}

function parsePluginDescriptor(content: string): PluginDescriptor {
  const descriptor: { -readonly [K in keyof PluginDescriptor]: string } = {
    name: "",
    description: "",
    author: "",
    version: "",
    script: "",
  };
  for (const line of content.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (
      trimmed.length === 0 ||
      trimmed.startsWith(";") ||
      trimmed.startsWith("#") ||
      trimmed.startsWith("[")
    ) {
      continue;
    }
    const equalsIndex = trimmed.indexOf("=");
    if (equalsIndex < 0) {
      continue;
    }
    const key = trimmed.slice(0, equalsIndex).trim();
    const rawValue = trimmed.slice(equalsIndex + 1).trim();
    const value = rawValue.startsWith('"') ? unquoteSimple(rawValue) : rawValue;
    if (
      key === "name" ||
      key === "description" ||
      key === "author" ||
      key === "version" ||
      key === "script"
    ) {
      descriptor[key] = value;
    }
  }
  return descriptor;
}

function pluginDescriptorWithinBounds(descriptor: PluginDescriptor): boolean {
  const values: readonly string[] = [
    descriptor.name,
    descriptor.description,
    descriptor.author,
    descriptor.version,
    descriptor.script,
  ];
  for (const value of values) {
    if (Buffer.byteLength(value, "utf8") > GODOT_LIMITS.maxProjectDescriptorValueBytes) {
      return false;
    }
  }
  return true;
}

function unquoteSimple(raw: string): string {
  if (raw.length >= 2 && raw.startsWith('"') && raw.endsWith('"')) {
    return raw.slice(1, -1);
  }
  return raw;
}

async function scanGDExtensions(
  options: ContentInventoryOptions,
  canonicalRoot: string,
  budget: TraversalBudget,
  files: readonly string[],
  warnings: SafeDiagnostic[],
  fsOps: GodotProjectFsOps,
): Promise<readonly GodotGDExtensionSummary[]> {
  const summaries: GodotGDExtensionSummary[] = [];
  for (const file of files) {
    if (budget.exhausted) {
      break;
    }
    budget.checkCancelled(options.signal);
    if (!budget.isWithinDeadline()) {
      break;
    }
    budget.descriptorsParsed += 1;
    if (budget.descriptorsParsed > budget.maxDescriptorsParsed) {
      budget.stop("descriptor-limit");
      warnings.push({
        severity: "warning",
        message:
          "The number of parsed project descriptors exceeded the bound (maxProjectDescriptorsParsed); the GDExtension inventory is partial.",
      });
      break;
    }
    if (budget.bytesRead >= budget.maxReadBytes) {
      budget.stop("bytes-limit");
      warnings.push({
        severity: "warning",
        message:
          "The total read byte budget (maxProjectTotalReadBytes) was exhausted; the GDExtension inventory is partial.",
      });
      break;
    }
    const read = await readBoundedProjectFile({
      canonicalRoot,
      path: file,
      maxBytes: GODOT_LIMITS.maxGDExtensionDescriptorBytes,
      fsOps,
    });
    if (!read.ok) {
      if (read.reason === "oversized") {
        warnings.push({
          severity: "warning",
          message: `The GDExtension descriptor ${workspaceRelative(canonicalRoot, file)} exceeds the size limit and was skipped.`,
        });
      } else if (read.reason === "symlink") {
        warnings.push({
          severity: "warning",
          message: `The GDExtension descriptor ${workspaceRelative(canonicalRoot, file)} is a symbolic link and was skipped.`,
        });
      } else if (read.reason === "outside" || read.reason === "changed") {
        warnings.push({
          severity: "warning",
          message: `The GDExtension descriptor ${workspaceRelative(canonicalRoot, file)} could not be safely resolved and was skipped.`,
        });
      }
      continue;
    }
    if (!budget.consumeBytes(read.bytesRead)) {
      warnings.push({
        severity: "warning",
        message:
          "The total read byte budget (maxProjectTotalReadBytes) was exhausted; the GDExtension inventory is partial.",
      });
      break;
    }
    const { compatibilityMinimum, libraryTargets } = parseGDExtensionDescriptor(read.content);
    if (
      (compatibilityMinimum !== null &&
        Buffer.byteLength(compatibilityMinimum, "utf8") >
          GODOT_LIMITS.maxProjectDescriptorValueBytes) ||
      libraryTargets.some(
        (target) => Buffer.byteLength(target, "utf8") > GODOT_LIMITS.maxProjectDescriptorValueBytes,
      )
    ) {
      warnings.push({
        severity: "warning",
        message: `The GDExtension descriptor ${workspaceRelative(canonicalRoot, file)} contains a value exceeding the descriptor value bound (maxProjectDescriptorValueBytes) and was skipped.`,
      });
      continue;
    }
    const descriptorDirectory = dirname(file);
    let libraryFilesExist = false;
    let escapesThroughSymlinks = false;
    const maxTargets = GODOT_LIMITS.maxGDExtensionTargetsPerDescriptor;
    for (let index = 0; index < libraryTargets.length; index += 1) {
      if (budget.exhausted) {
        break;
      }
      budget.checkCancelled(options.signal);
      if (!budget.isWithinDeadline()) {
        break;
      }
      if (index >= maxTargets) {
        // Not assessed: the descriptor stays visible but is marked unsafe.
        escapesThroughSymlinks = true;
        warnings.push({
          severity: "warning",
          message: `The GDExtension descriptor ${workspaceRelative(canonicalRoot, file)} declares more library targets than the bound (maxGDExtensionTargetsPerDescriptor); the remaining targets were not assessed.`,
        });
        break;
      }
      const target = libraryTargets[index] ?? "";
      // The target is attacker-controlled: reject unsafe spellings lexically
      // before constructing any path or calling lstat/realpath on it.
      const targetReference = target.startsWith("res://") ? target.slice("res://".length) : target;
      const verdict = validateProjectRelativePath(
        targetReference,
        GODOT_LIMITS.maxResReferencePathBytes,
      );
      if (!verdict.ok) {
        escapesThroughSymlinks = true;
        warnings.push({
          severity: "warning",
          message: `The GDExtension descriptor ${workspaceRelative(canonicalRoot, file)} declares a library target (${target}) that is not a contained project path; it was not inspected.`,
        });
        continue;
      }
      const targetBase = target.startsWith("res://") ? canonicalRoot : descriptorDirectory;
      const targetPath = join(targetBase, verdict.value);
      const verified = await verifyProjectPathContainment(canonicalRoot, targetPath, fsOps);
      if (!verified.ok) {
        if (verified.reason === "outside" || verified.reason === "symlink") {
          escapesThroughSymlinks = true;
          warnings.push({
            severity: "warning",
            message: `The GDExtension descriptor ${workspaceRelative(canonicalRoot, file)} declares a library target (${target}) that cannot be safely resolved inside the workspace; it was not inspected.`,
          });
        }
        continue;
      }
      let targetMetadata;
      try {
        targetMetadata = await fsOps.lstat(targetPath);
      } catch {
        continue;
      }
      if (targetMetadata.isFile()) {
        libraryFilesExist = true;
      }
      const canonical = await fsOps.realpath(targetPath).catch(() => null);
      if (canonical === null || !samePathIdentity(canonical, verified.canonicalPath)) {
        escapesThroughSymlinks = true;
      }
    }
    if (!budget.addInventoryItem()) {
      warnings.push({
        severity: "warning",
        message:
          "The executable-content inventory exceeded its item bound (maxProjectInventoryItems); the GDExtension inventory is partial.",
      });
      break;
    }
    summaries.push({
      path: workspaceRelative(canonicalRoot, file),
      compatibilityMinimum,
      libraryTargets,
      libraryFilesExist,
      escapesThroughSymlinks,
    });
  }
  return summaries;
}

function parseGDExtensionDescriptor(content: string): {
  readonly compatibilityMinimum: string | null;
  readonly libraryTargets: readonly string[];
} {
  let section = "";
  let compatibilityMinimum: string | null = null;
  const libraryTargets: string[] = [];
  for (const line of content.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
      section = trimmed.slice(1, -1).trim();
      continue;
    }
    const equalsIndex = trimmed.indexOf("=");
    if (equalsIndex < 0) {
      continue;
    }
    const key = trimmed.slice(0, equalsIndex).trim();
    const rawValue = trimmed.slice(equalsIndex + 1).trim();
    const value = unquoteSimple(rawValue);
    if (section === "configuration" && key === "compatibility_minimum") {
      compatibilityMinimum = value;
    }
    if (section === "entry" && key.length > 0 && value.length > 0) {
      libraryTargets.push(value);
    }
  }
  return { compatibilityMinimum, libraryTargets };
}

function defaultReadHead(fsOps: GodotProjectFsOps): GodotProjectReadHead {
  return async (
    path: string,
    maxBytes: number,
  ): Promise<
    | { readonly ok: true; readonly content: string; readonly bytesRead: number }
    | { readonly ok: false }
  > => {
    try {
      const handle = await fsOps.open(path);
      try {
        const buffer = Buffer.alloc(Math.min(maxBytes, GODOT_LIMITS.maxToolScriptHeadBytes));
        let total = 0;
        while (total < buffer.length) {
          const { bytesRead } = await handle.read(buffer, total, buffer.length - total, total);
          if (bytesRead === 0) {
            break; // EOF
          }
          total += bytesRead;
        }
        return {
          ok: true,
          content: buffer.subarray(0, total).toString("utf8"),
          bytesRead: total,
        };
      } finally {
        await handle.close().catch(() => undefined);
      }
    } catch {
      return { ok: false };
    }
  };
}

function workspaceRelative(root: string, path: string): string {
  return relative(root, path).split(sep).join("/");
}
