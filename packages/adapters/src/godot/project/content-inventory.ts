import { lstat, open, readdir, realpath } from "node:fs/promises";
import { basename, dirname, join, relative, sep } from "node:path";
import {
  GODOT_LIMITS,
  type GodotExecutableContentInventory,
  type GodotGDExtensionSummary,
  type GodotPluginSummary,
  type SafeDiagnostic,
} from "@solaris/core";
import { containsCodeToken } from "./lexical.js";
import { scanProjectFiles, type BoundedScanResult } from "./bounded-scan.js";

export interface ContentInventoryOptions {
  readonly workspaceRoot: string;
  readonly signal?: AbortSignal;
  readonly enabledPlugins: readonly string[];
  readonly autoloadCount: number;
  /** Optional pre-computed traversal; deterministic tests inject a stub. */
  readonly scan?: Promise<BoundedScanResult>;
  readonly readHead?: (
    path: string,
    maxBytes: number,
  ) => Promise<{ readonly ok: true; readonly content: string } | { readonly ok: false }>;
}

/**
 * Static executable-content inventory. Nothing is loaded or run: `@tool`
 * markers are detected lexically near the start of bounded `.gd` files,
 * `plugin.cfg` descriptors are parsed conservatively, import plugins are a
 * heuristic based on `extends EditorImportPlugin`, GDExtension descriptors
 * are read without loading native libraries, and all traversal is bounded
 * and symlink-safe.
 */
export async function inventoryExecutableContent(options: ContentInventoryOptions): Promise<{
  readonly inventory: GodotExecutableContentInventory;
  readonly warnings: readonly SafeDiagnostic[];
}> {
  const warnings: SafeDiagnostic[] = [];
  const scan =
    options.scan ??
    scanProjectFiles({
      workspaceRoot: options.workspaceRoot,
      ...(options.signal === undefined ? {} : { signal: options.signal }),
      onlyGdFiles: false,
      includeDotnet: true,
      includeGDExtensions: true,
    });
  const result = await scan;
  const toolScripts: string[] = [];
  const dotnetProjectFiles: string[] = [];
  const gdextensionFiles: string[] = [];
  let sourceBytesInspected = 0;
  let byteBudgetExhausted = false;
  for (const file of result.files) {
    const name = basename(file).toLowerCase();
    if (name.endsWith(".gd")) {
      if (sourceBytesInspected >= GODOT_LIMITS.maxSourceBytesInspected) {
        byteBudgetExhausted = true;
        break;
      }
      const head = await (options.readHead ?? readHeadFile)(
        file,
        GODOT_LIMITS.maxToolScriptHeadBytes,
      );
      sourceBytesInspected += head.ok ? head.content.length : 0;
      if (head.ok && containsCodeToken(head.content, "@tool")) {
        toolScripts.push(relative(options.workspaceRoot, file).split(sep).join("/"));
      }
    } else if (name.endsWith(".csproj") || name.endsWith(".sln")) {
      dotnetProjectFiles.push(relative(options.workspaceRoot, file).split(sep).join("/"));
    } else if (name.endsWith(".gdextension")) {
      gdextensionFiles.push(file);
    }
  }
  if (byteBudgetExhausted) {
    warnings.push({
      severity: "warning",
      message:
        "The source inspection byte budget was exhausted; the tool-script inventory is partial.",
    });
  }
  const editorPlugins = await scanEditorPlugins(options, warnings);
  const gdextensionDescriptors = await scanGDExtensions(options, gdextensionFiles, warnings);
  const inventory: GodotExecutableContentInventory = {
    toolScripts,
    editorPlugins,
    importPlugins: editorPlugins
      .filter((plugin) => plugin.importPluginHeuristic)
      .map((plugin) => plugin.path),
    gdextensionDescriptors,
    autoloadCount: options.autoloadCount,
    dotnetProjectFiles,
    scanTruncated: result.truncated || byteBudgetExhausted,
  };
  return { inventory, warnings };
}

async function scanEditorPlugins(
  options: ContentInventoryOptions,
  warnings: SafeDiagnostic[],
): Promise<readonly GodotPluginSummary[]> {
  const addonsDirectory = join(options.workspaceRoot, "addons");
  let entries: readonly { readonly name: string; readonly isDirectory: () => boolean }[];
  try {
    entries = await readdir(addonsDirectory, { withFileTypes: true });
  } catch {
    return [];
  }
  const plugins: GodotPluginSummary[] = [];
  for (const entry of entries) {
    if (!entry.isDirectory() || entry.name.startsWith(".")) {
      continue;
    }
    const pluginDirectory = join(addonsDirectory, entry.name);
    const pluginFile = join(pluginDirectory, "plugin.cfg");
    let metadata;
    try {
      metadata = await lstat(pluginFile);
    } catch {
      continue;
    }
    if (metadata.isSymbolicLink() || !metadata.isFile()) {
      warnings.push({
        severity: "warning",
        message: `The editor plugin descriptor addons/${entry.name}/plugin.cfg is not a regular file and was skipped.`,
      });
      continue;
    }
    if (metadata.size > GODOT_LIMITS.maxPluginDescriptorBytes) {
      warnings.push({
        severity: "warning",
        message: `The editor plugin descriptor addons/${entry.name}/plugin.cfg exceeds the size limit and was skipped.`,
      });
      continue;
    }
    const parsed = await readPluginDescriptor(pluginFile);
    if (parsed === null) {
      continue;
    }
    const pluginPath = `addons/${entry.name}`;
    const enabled = options.enabledPlugins.includes(`res://${pluginPath}`);
    const scriptPath = parsed.script;
    const language = scriptPath.endsWith(".cs")
      ? ("dotnet" as const)
      : scriptPath.endsWith(".gd")
        ? ("gdscript" as const)
        : ("unknown" as const);
    let importPluginHeuristic = false;
    if (language === "gdscript" && scriptPath.length > 0) {
      const scriptFile = join(pluginDirectory, scriptPath.replace(/^res:\/\//, ""));
      const head = await (options.readHead ?? readHeadFile)(
        scriptFile,
        GODOT_LIMITS.maxToolScriptHeadBytes,
      );
      if (head.ok && containsCodeToken(head.content, "extends EditorImportPlugin")) {
        importPluginHeuristic = true;
      }
    }
    plugins.push({
      path: pluginPath,
      name: parsed.name,
      description: parsed.description,
      author: parsed.author,
      version: parsed.version,
      scriptPath,
      language,
      enabled,
      importPluginHeuristic,
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

async function readPluginDescriptor(path: string): Promise<PluginDescriptor | null> {
  let content: string;
  try {
    const { readFile } = await import("node:fs/promises");
    content = await readFile(path, "utf8");
  } catch {
    return null;
  }
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

function unquoteSimple(raw: string): string {
  if (raw.length >= 2 && raw.startsWith('"') && raw.endsWith('"')) {
    return raw.slice(1, -1);
  }
  return raw;
}

async function scanGDExtensions(
  options: ContentInventoryOptions,
  files: readonly string[],
  warnings: SafeDiagnostic[],
): Promise<readonly GodotGDExtensionSummary[]> {
  const summaries: GodotGDExtensionSummary[] = [];
  for (const file of files) {
    let metadata;
    try {
      metadata = await lstat(file);
    } catch {
      continue;
    }
    if (metadata.isSymbolicLink() || !metadata.isFile()) {
      continue;
    }
    if (metadata.size > GODOT_LIMITS.maxGDExtensionDescriptorBytes) {
      warnings.push({
        severity: "warning",
        message: `The GDExtension descriptor ${relative(options.workspaceRoot, file)} exceeds the size limit and was skipped.`,
      });
      continue;
    }
    let content: string;
    try {
      const { readFile } = await import("node:fs/promises");
      content = await readFile(file, "utf8");
    } catch {
      continue;
    }
    const { compatibilityMinimum, libraryTargets } = parseGDExtensionDescriptor(content);
    const descriptorDirectory = dirname(file);
    let libraryFilesExist = false;
    let escapesThroughSymlinks = false;
    const workspacePrefix = options.workspaceRoot.endsWith(sep)
      ? options.workspaceRoot
      : `${options.workspaceRoot}${sep}`;
    for (const target of libraryTargets) {
      const targetPath = target.startsWith("res://")
        ? join(options.workspaceRoot, target.slice("res://".length))
        : join(descriptorDirectory, target.replace(/^\.\//, ""));
      let targetMetadata;
      try {
        targetMetadata = await lstat(targetPath);
      } catch {
        continue;
      }
      if (targetMetadata.isFile()) {
        libraryFilesExist = true;
      }
      try {
        const canonical = await realpath(targetPath);
        const canonicalParent = await realpath(dirname(targetPath));
        const expectedCanonical = join(canonicalParent, basename(targetPath));
        const inWorkspace =
          canonical.startsWith(workspacePrefix) || canonical === options.workspaceRoot;
        if (canonical !== expectedCanonical || !inWorkspace) {
          escapesThroughSymlinks = true;
        }
      } catch {
        escapesThroughSymlinks = true;
      }
    }
    summaries.push({
      path: relative(options.workspaceRoot, file).split(sep).join("/"),
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

async function readHeadFile(
  path: string,
  maxBytes: number,
): Promise<{ readonly ok: true; readonly content: string } | { readonly ok: false }> {
  try {
    const handle = await open(path, "r");
    try {
      const buffer = Buffer.alloc(Math.min(maxBytes, GODOT_LIMITS.maxToolScriptHeadBytes));
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
      return { ok: true, content: buffer.subarray(0, bytesRead).toString("utf8") };
    } finally {
      await handle.close().catch(() => undefined);
    }
  } catch {
    return { ok: false };
  }
}
