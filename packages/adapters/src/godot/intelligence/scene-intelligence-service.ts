import { createHash } from "node:crypto";
import { readdir, stat } from "node:fs/promises";
import { join } from "node:path";
import {
  GODOT_SCENE_LIMITS,
  REVIEW_CONTEXT_LIMITS,
  analyzeImpact,
  buildSceneNodeTree,
  createGodotRelationshipIndex,
  parseGodotResource,
  parseGodotScene,
  resolveResPath,
  type GodotDependencyEdge,
  type GodotDependencyResult,
  type GodotImpactRequest,
  type GodotImpactResult,
  type GodotProjectRelationshipResult,
  type GodotRelationshipEntry,
  type GodotRelationshipIndex,
  type GodotRelationshipKind,
  type GodotResourceInspectionResult,
  type GodotResourceModel,
  type GodotSceneEvidenceView,
  type GodotSceneInspectionResult,
  type GodotSceneIntelligence,
  type GodotSceneIntelligenceSupport,
  type GodotSceneModel,
  type ImpactEdge,
  type ImpactRelationKind,
  type ImpactRelationshipSource,
  type ImpactSignalConnection,
  type WorkspaceRevisionHandle,
  type WorkspaceRevisionRegistry,
} from "@solaris/core";
import { readFileBounded } from "../../fs/file-read.js";
import {
  describeFsError,
  findExcludedComponent,
  resolveWorkspacePath,
} from "../../tools/workspace/workspace-path.js";
import { decodeUtf8, looksBinary } from "../../tools/workspace/text.js";
import { readProjectFile } from "../project/project-files.js";
import { scanProjectFile } from "../project/project-scanner.js";

/**
 * Godot scene/resource intelligence service (Stage 3 milestone 8).
 *
 * The single application-owned subsystem for current parsed Godot
 * semantic state. All inspection is STATIC: bounded workspace reads only —
 * no Godot process, no `@tool` execution, no plugin activation, no project
 * loading, no mutation, no checkpoint. Every result binds to the exact
 * workspace revision of the file state that was read; a changed file makes
 * the previous model historical evidence, never current truth.
 */

export interface SceneIntelligenceDependencies {
  readonly workspaceRoot: string;
  /** Session revision registry (opaque handles; the host owns authority). */
  readonly revisions: WorkspaceRevisionRegistry;
  /**
   * Record a bounded inspection observation (context projection /
   * observability). The service stays the single owner of parsed state;
   * observers only consume disposable views.
   */
  readonly onInspection?: (view: GodotSceneEvidenceView) => void;
  /** Bounded read limit; defaults to GODOT_SCENE_LIMITS.maxDocumentBytes. */
  readonly maxDocumentBytes?: number;
}

type ReadStatus = "ok" | "not_found" | "unreadable" | "unsupported" | "denied" | "failed";

interface ReadOutcome {
  readonly status: ReadStatus;
  readonly message: string | null;
  /** Workspace-relative path (when resolution succeeded). */
  readonly relativePath: string | null;
  readonly content: string | null;
  readonly revision: WorkspaceRevisionHandle | null;
}

const SCENE_EXTENSIONS = [".tscn"];
const RESOURCE_EXTENSIONS = [".tres", ".theme"];
const EXCLUDED_DIRECTORIES = ["node_modules", ".git", "dist", "coverage"];

export function createGodotSceneIntelligence(
  dependencies: SceneIntelligenceDependencies,
): GodotSceneIntelligence {
  const { workspaceRoot, revisions } = dependencies;
  const maxDocumentBytes = dependencies.maxDocumentBytes ?? GODOT_SCENE_LIMITS.maxDocumentBytes;
  // The relationship index is application-owned: this service is its
  // single writer and reader. No other component builds its own maps.
  const index: GodotRelationshipIndex = createGodotRelationshipIndex();

  async function readDocument(path: string): Promise<ReadOutcome> {
    const resolved = await resolveWorkspacePath(workspaceRoot, path);
    if (resolved.status === "rejected") {
      // Distinguish an out-of-workspace path (policy violation) from a
      // missing file (honest not_found): a lexically contained relative
      // path that fails canonical resolution simply does not exist.
      if (isLexicallyContainedPath(path)) {
        return {
          status: "not_found",
          message: "File does not exist in the workspace.",
          relativePath: null,
          content: null,
          revision: null,
        };
      }
      return {
        status: "denied",
        message: resolved.message,
        relativePath: null,
        content: null,
        revision: null,
      };
    }
    const excluded = findExcludedComponent(resolved.workspaceRelativePath, EXCLUDED_DIRECTORIES);
    if (excluded !== null) {
      return {
        status: "denied",
        message: `Path is inside the excluded directory ${excluded}.`,
        relativePath: resolved.workspaceRelativePath,
        content: null,
        revision: null,
      };
    }
    let stats;
    try {
      stats = await stat(resolved.absolutePath);
    } catch (error: unknown) {
      return {
        status: "not_found",
        message: `Cannot inspect file: ${describeFsError(error)}`,
        relativePath: resolved.workspaceRelativePath,
        content: null,
        revision: null,
      };
    }
    if (!stats.isFile()) {
      return {
        status: "failed",
        message: "Target is not a regular file.",
        relativePath: resolved.workspaceRelativePath,
        content: null,
        revision: null,
      };
    }
    if (stats.size > maxDocumentBytes) {
      return {
        status: "unreadable",
        message: `File is too large (${stats.size} bytes; limit ${maxDocumentBytes}).`,
        relativePath: resolved.workspaceRelativePath,
        content: null,
        revision: null,
      };
    }
    const buffer = await readFileBounded(resolved.absolutePath, maxDocumentBytes);
    if (buffer === null) {
      return {
        status: "unreadable",
        message: `Cannot read file: it is missing, not a regular file, or exceeds the ${maxDocumentBytes}-byte limit.`,
        relativePath: resolved.workspaceRelativePath,
        content: null,
        revision: null,
      };
    }
    if (looksBinary(buffer)) {
      return {
        status: "unreadable",
        message: "File appears to be binary.",
        relativePath: resolved.workspaceRelativePath,
        content: null,
        revision: null,
      };
    }
    const text = decodeUtf8(buffer);
    if (text === null) {
      return {
        status: "unreadable",
        message: "File is not valid UTF-8 text.",
        relativePath: resolved.workspaceRelativePath,
        content: null,
        revision: null,
      };
    }
    const sha256 = createHash("sha256").update(buffer).digest("hex");
    const revision = revisions.issue(resolved.workspaceRelativePath, sha256);
    revisions.observeRead(resolved.workspaceRelativePath, revision, "structural");
    return {
      status: "ok",
      message: null,
      relativePath: resolved.workspaceRelativePath,
      content: text,
      revision,
    };
  }

  function gateScene(
    path: string,
  ): { readonly ok: true } | { readonly ok: false; readonly message: string } {
    const lower = path.toLowerCase();
    if (SCENE_EXTENSIONS.some((extension) => lower.endsWith(extension))) {
      return { ok: true };
    }
    return { ok: false, message: "godot.inspect_scene requires a .tscn path." };
  }

  function gateResource(
    path: string,
  ): { readonly ok: true } | { readonly ok: false; readonly message: string } {
    const lower = path.toLowerCase();
    if (lower.endsWith(".tscn")) {
      return {
        ok: false,
        message: "godot.inspect_resource does not accept .tscn paths; use godot.inspect_scene.",
      };
    }
    if (RESOURCE_EXTENSIONS.some((extension) => lower.endsWith(extension))) {
      return { ok: true };
    }
    return {
      ok: false,
      message: "godot.inspect_resource accepts .tres and .theme resource files only.",
    };
  }

  async function inspectScene(request: {
    readonly path: string;
  }): Promise<GodotSceneInspectionResult> {
    const gate = gateScene(request.path);
    if (!gate.ok) {
      return {
        status: "unsupported",
        message: gate.message,
        path: request.path,
        revision: null,
        document: null,
        tree: null,
      };
    }
    const read = await readDocument(request.path);
    if (read.status !== "ok" || read.content === null || read.relativePath === null) {
      return {
        status: read.status,
        message: read.message,
        path: request.path,
        revision: read.revision,
        document: null,
        tree: null,
      };
    }
    const document = parseGodotScene(read.content, read.relativePath, { revision: read.revision });
    const tree = document.document === null ? null : buildSceneNodeTree(document.document);
    if (document.document !== null) {
      recordSceneRelationships(document.document);
    }
    dependencies.onInspection?.({
      path: read.relativePath,
      revision: read.revision,
      kind: "scene",
      status: document.status,
      summary: summarizeScene(document.document),
      evidenceId: null,
    });
    return {
      status: "ok",
      message: null,
      path: read.relativePath,
      revision: read.revision,
      document,
      tree,
    };
  }

  async function inspectResource(request: {
    readonly path: string;
  }): Promise<GodotResourceInspectionResult> {
    const gate = gateResource(request.path);
    if (!gate.ok) {
      return {
        status: "unsupported",
        message: gate.message,
        path: request.path,
        revision: null,
        document: null,
      };
    }
    const read = await readDocument(request.path);
    if (read.status !== "ok" || read.content === null || read.relativePath === null) {
      return {
        status: read.status,
        message: read.message,
        path: request.path,
        revision: read.revision,
        document: null,
      };
    }
    const document = parseGodotResource(read.content, read.relativePath, {
      revision: read.revision,
    });
    if (document.document !== null) {
      recordResourceRelationships(document.document);
    }
    dependencies.onInspection?.({
      path: read.relativePath,
      revision: read.revision,
      kind: "resource",
      status: document.status,
      summary: summarizeResource(document.document),
      evidenceId: null,
    });
    return {
      status: "ok",
      message: null,
      path: read.relativePath,
      revision: read.revision,
      document,
    };
  }

  async function dependenciesFor(request: {
    readonly path: string;
  }): Promise<GodotDependencyResult> {
    const gate = gateScene(request.path).ok
      ? ({ ok: true, kind: "scene" } as const)
      : gateResource(request.path).ok
        ? ({ ok: true, kind: "resource" } as const)
        : ({
            ok: false,
            message: "godot.dependencies accepts .tscn and .tres paths only.",
          } as const);
    if (!gate.ok) {
      return {
        status: "unsupported",
        message: gate.message,
        rootPath: request.path,
        revision: null,
        edges: [],
        referrers: [],
        filesVisited: 0,
        truncatedDepth: false,
        truncatedFiles: false,
        cycleDetected: false,
      };
    }
    const edges: GodotDependencyEdge[] = [];
    const visited = new Set<string>();
    const stack: string[] = [];
    let filesVisited = 0;
    let truncatedDepth = false;
    let truncatedFiles = false;
    let cycleDetected = false;
    let cyclePath: readonly string[] | undefined;
    let rootRevision: WorkspaceRevisionHandle | null = null;

    const visit = async (
      path: string,
      depth: number,
      kind: "scene" | "resource",
    ): Promise<{ readonly status: ReadStatus; readonly message: string | null } | null> => {
      if (filesVisited >= GODOT_SCENE_LIMITS.maxDependencyFiles) {
        truncatedFiles = true;
        return null;
      }
      if (stack.includes(path)) {
        // A path already on the current walk stack is a cycle; stop this
        // branch safely and report the cycle path.
        cycleDetected = true;
        cyclePath = [...stack.slice(stack.indexOf(path)), path];
        return null;
      }
      if (visited.has(path)) {
        return null;
      }
      visited.add(path);
      stack.push(path);
      filesVisited += 1;
      const read = await readDocument(path);
      if (read.status !== "ok" || read.content === null || read.relativePath === null) {
        stack.pop();
        if (depth === 0) {
          // The root document itself could not be read: surface the real
          // failure instead of an empty "ok" result.
          return { status: read.status, message: read.message };
        }
        return null;
      }
      if (depth === 0) {
        rootRevision = read.revision;
      }
      const document =
        kind === "scene"
          ? parseGodotScene(read.content, read.relativePath, { revision: read.revision })
          : parseGodotResource(read.content, read.relativePath, { revision: read.revision });
      if (document.document !== null) {
        const model = document.document;
        const next: {
          readonly path: string;
          readonly kind: GodotRelationshipKind;
          readonly uid?: string;
        }[] = [];
        if (kind === "scene") {
          const scene = model as GodotSceneModel;
          if (scene.baseScene?.resolvedPath !== undefined) {
            next.push({
              path: scene.baseScene.resolvedPath,
              kind: "scene_inherits",
              ...(scene.baseScene.resource.uid === undefined
                ? {}
                : { uid: scene.baseScene.resource.uid }),
            });
          }
          for (const node of scene.nodes) {
            if (node.instance?.resolvedPath !== undefined) {
              next.push({
                path: node.instance.resolvedPath,
                kind: "scene_instances",
                ...(node.instance.resource.uid === undefined
                  ? {}
                  : { uid: node.instance.resource.uid }),
              });
            }
            if (node.script?.resolvedPath !== undefined) {
              next.push({
                path: node.script.resolvedPath,
                kind: "scene_uses_script",
                ...(node.script.resource.uid === undefined
                  ? {}
                  : { uid: node.script.resource.uid }),
              });
            }
          }
        }
        for (const resource of model.externalResources) {
          if (resource.path === undefined) {
            continue;
          }
          const resolved = resolveResPath(resource.path);
          if (resolved.ok) {
            next.push({
              path: resolved.relativePath,
              kind: "resource_references",
              ...(resource.uid === undefined ? {} : { uid: resource.uid }),
            });
          }
        }
        for (const target of next) {
          edges.push({
            kind: target.kind,
            sourcePath: read.relativePath,
            targetPath: target.path,
            ...(target.uid === undefined ? {} : { targetUid: target.uid }),
            depth,
          });
          const lower = target.path.toLowerCase();
          const targetKind = lower.endsWith(".tscn") ? "scene" : "resource";
          if (targetKind === "scene" || lower.endsWith(".tres") || lower.endsWith(".theme")) {
            if (depth + 1 <= GODOT_SCENE_LIMITS.maxDependencyDepth) {
              const failure = await visit(target.path, depth + 1, targetKind);
              if (failure !== null) {
                stack.pop();
                return failure;
              }
            } else {
              truncatedDepth = true;
            }
          }
        }
      }
      stack.pop();
      return null;
    };

    const failure = await visit(request.path, 0, gate.kind);
    if (failure !== null) {
      return {
        status: failure.status,
        message: failure.message,
        rootPath: request.path,
        revision: null,
        edges: [],
        referrers: [],
        filesVisited: 0,
        truncatedDepth: false,
        truncatedFiles: false,
        cycleDetected: false,
      };
    }
    const referrers = index.referrersOf(request.path).map((entry) => ({
      sourcePath: entry.sourcePath,
      kind: entry.kind,
      sourceRevision: entry.sourceRevision,
      stale: index.isStale(entry, revisions.currentRevision(entry.sourcePath)),
    }));
    return {
      status: "ok",
      message: null,
      rootPath: request.path,
      revision: rootRevision,
      edges,
      referrers,
      filesVisited,
      truncatedDepth,
      truncatedFiles,
      cycleDetected,
      ...(cyclePath === undefined ? {} : { cyclePath }),
    };
  }

  async function projectRelationships(): Promise<GodotProjectRelationshipResult> {
    const read = await readProjectFile(workspaceRoot);
    if (!read.ok) {
      return {
        status: "no_project",
        message: read.message,
        mainScene: null,
        autoloads: [],
        inputActions: [],
        diagnostics: [],
      };
    }
    // Register the project file revision (current-revision tracking).
    revisions.issue("project.godot", read.sha256);
    const scan = scanProjectFile(read.content);
    // Main scene: resolve res:// within workspace containment; verify existence.
    let mainScene: GodotProjectRelationshipResult["mainScene"] = null;
    if (scan.mainScene !== null) {
      const resolved = resolveResPath(scan.mainScene);
      if (resolved.ok) {
        const sceneRead = await readDocument(resolved.relativePath);
        mainScene = {
          path: resolved.relativePath,
          revision: sceneRead.revision,
          exists: sceneRead.status === "ok",
        };
      }
    }
    const autoloads = scan.autoloads.map((autoload) => {
      const target = autoload.target;
      const raw = target.startsWith("*") ? target.slice(1) : target;
      const resolved = raw.startsWith("res://") ? resolveResPath(raw) : null;
      return {
        name: autoload.name,
        path: resolved?.ok === true ? resolved.relativePath : raw.replace(/^res:\/\//, ""),
        enabled: !autoload.name.startsWith("*_"),
        targetKind: classifyAutoloadTarget(resolved?.ok === true ? resolved.relativePath : raw),
        target: target.slice(0, GODOT_SCENE_LIMITS.maxRawValueLength),
      };
    });
    return {
      status: "ok",
      message: null,
      mainScene,
      autoloads,
      inputActions: scan.inputActions,
      diagnostics: scan.warnings.map((warning) => ({
        code: "resource.unbalanced_value" as const,
        severity: "warning" as const,
        message: warning.message,
      })),
    };
  }

  function recordSceneRelationships(model: GodotSceneModel): void {
    const entries: GodotRelationshipEntry[] = [];
    const add = (
      kind: GodotRelationshipKind,
      targetPath: string,
      targetUid: string | undefined,
    ): void => {
      entries.push({
        sourcePath: model.path,
        sourceRevision: model.revision,
        kind,
        targetPath,
        ...(targetUid === undefined ? {} : { targetUid }),
      });
    };
    if (model.baseScene?.resolvedPath !== undefined) {
      add("scene_inherits", model.baseScene.resolvedPath, model.baseScene.resource.uid);
    }
    for (const node of model.nodes) {
      if (node.instance?.resolvedPath !== undefined) {
        add("scene_instances", node.instance.resolvedPath, node.instance.resource.uid);
      }
      if (node.script?.resolvedPath !== undefined) {
        add("scene_uses_script", node.script.resolvedPath, node.script.resource.uid);
      }
    }
    for (const resource of model.externalResources) {
      if (resource.path === undefined) {
        continue;
      }
      const resolved = resolveResPath(resource.path);
      if (!resolved.ok) {
        continue;
      }
      const isScene =
        resource.type === "PackedScene" || resolved.relativePath.toLowerCase().endsWith(".tscn");
      const isScript =
        resource.type === "Script" || resolved.relativePath.toLowerCase().endsWith(".gd");
      if (isScene || isScript) {
        continue; // already recorded through base/instance/script relationships
      }
      add("resource_references", resolved.relativePath, resource.uid);
    }
    index.record(model.path, entries);
  }

  function recordResourceRelationships(model: GodotResourceModel): void {
    const entries: GodotRelationshipEntry[] = [];
    if (model.script?.resolvedPath !== undefined) {
      entries.push({
        sourcePath: model.path,
        sourceRevision: model.revision,
        kind: "resource_references",
        targetPath: model.script.resolvedPath,
        ...(model.script.resource.uid === undefined
          ? {}
          : { targetUid: model.script.resource.uid }),
      });
    }
    for (const resource of model.externalResources) {
      if (resource.path === undefined) {
        continue;
      }
      const resolved = resolveResPath(resource.path);
      if (resolved.ok) {
        entries.push({
          sourcePath: model.path,
          sourceRevision: model.revision,
          kind: "resource_references",
          targetPath: resolved.relativePath,
          ...(resource.uid === undefined ? {} : { targetUid: resource.uid }),
        });
      }
    }
    index.record(model.path, entries);
  }

  async function impactSignalConnections(path: string): Promise<readonly ImpactSignalConnection[]> {
    if (!path.toLowerCase().endsWith(".tscn")) {
      return [];
    }
    const read = await readDocument(path);
    if (read.status !== "ok" || read.content === null || read.relativePath === null) {
      return [];
    }
    const parsed = parseGodotScene(read.content, read.relativePath, { revision: read.revision });
    if (parsed.document === null) {
      return [];
    }
    return parsed.document.connections.map((connection) => ({
      signal: connection.signal,
      sourceNode: connection.from,
      targetNode: connection.to,
      targetMethod: connection.method,
    }));
  }

  function impactEdgesOf(entries: readonly GodotRelationshipEntry[]): readonly ImpactEdge[] {
    const edges: ImpactEdge[] = [];
    for (const entry of entries) {
      const kind = IMPACT_KIND_BY_INDEX_KIND[entry.kind];
      if (kind === undefined) {
        continue;
      }
      edges.push({
        kind,
        fromPath: entry.sourcePath,
        toPath: entry.targetPath,
        stale: index.isStale(entry, revisions.currentRevision(entry.sourcePath)),
      });
    }
    return edges;
  }

  async function reviewContext(request: GodotImpactRequest): Promise<GodotImpactResult> {
    const project = await projectRelationships();
    const autoloads = project.status === "ok" ? project.autoloads : [];
    const mainScene = project.status === "ok" ? (project.mainScene?.path ?? null) : null;
    const source: ImpactRelationshipSource = {
      outgoing: (path) => impactEdgesOf(index.dependenciesOf(path)),
      incoming: (path) => impactEdgesOf(index.referrersOf(path)),
      signalConnections: impactSignalConnections,
      autoloadName: (path) => autoloads.find((autoload) => autoload.path === path)?.name ?? null,
      mainScene: () => mainScene,
      currentRevision: (path) => revisions.currentRevision(path),
      candidateTests: (path) => enumerateCandidateTestFiles(workspaceRoot, path),
    };
    try {
      const manifest = await analyzeImpact({
        taskId: request.taskId,
        taskContractRevision: request.taskContractRevision,
        changedPaths: request.changedPaths,
        source,
      });
      return { status: "ok", message: null, manifest };
    } catch (error: unknown) {
      return {
        status: "failed",
        message: `Impact analysis failed: ${describeFsError(error)}`,
        manifest: null,
      };
    }
  }

  return {
    inspectScene,
    inspectResource,
    dependencies: dependenciesFor,
    projectRelationships,
    reviewContext,
    support(): GodotSceneIntelligenceSupport {
      return { state: "ready" };
    },
  };
}

const IMPACT_KIND_BY_INDEX_KIND: Readonly<
  Partial<Record<GodotRelationshipKind, ImpactRelationKind>>
> = {
  scene_inherits: "scene_inheritance",
  scene_instances: "scene_instancing",
  scene_uses_script: "script_attachment",
  resource_references: "resource_dependency",
};

/**
 * Bounded candidate-test enumeration (Stage 3 milestone 9 §11): a
 * deterministic, capped workspace walk for `.gd` files whose path matches
 * test conventions (a `tests`/`test` directory segment, or `.test.` /
 * `.spec.` names). Candidates share the changed file's directory or live
 * under a `tests` tree. These are CANDIDATE surfaces by convention — never
 * verified coverage.
 */
async function enumerateCandidateTestFiles(
  workspaceRoot: string,
  changedPath: string,
): Promise<readonly string[]> {
  const changedDirectory = changedPath.includes("/")
    ? changedPath.slice(0, changedPath.lastIndexOf("/"))
    : ".";
  const changedStem = changedPath.includes("/")
    ? changedPath.slice(changedPath.lastIndexOf("/") + 1).replace(/\.[^.]+$/, "")
    : changedPath.replace(/\.[^.]+$/, "");
  const candidates: string[] = [];
  const stack: Array<{ readonly directory: string; readonly depth: number }> = [
    { directory: ".", depth: 0 },
  ];
  const MAX_DEPTH = 6;
  let inspected = 0;
  while (stack.length > 0) {
    const { directory, depth } = stack.pop()!;
    if (depth > MAX_DEPTH || inspected >= 512) {
      break;
    }
    let entries;
    try {
      entries = await readdir(join(workspaceRoot, directory), { withFileTypes: true });
    } catch {
      continue;
    }
    inspected += 1;
    for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
      if (candidates.length >= REVIEW_CONTEXT_LIMITS.maxCandidateTests) {
        break;
      }
      const relative = directory === "." ? entry.name : `${directory}/${entry.name}`;
      if (entry.isDirectory()) {
        if (EXCLUDED_DIRECTORIES.includes(entry.name)) {
          continue;
        }
        stack.push({ directory: relative, depth: depth + 1 });
        continue;
      }
      if (!entry.isFile() || !entry.name.toLowerCase().endsWith(".gd")) {
        continue;
      }
      const isTestName = /\.(test|spec)\.gd$/i.test(entry.name);
      const inTestsTree = /(^|\/)tests?(\/|$)/.test(relative);
      // Colocated test-convention file, or a test-tree file whose name
      // carries the changed surface's stem (path/module ownership + naming
      // conventions). Always CANDIDATE by convention — never verified.
      const colocated = directory === changedDirectory && isTestName;
      const stemMatch = inTestsTree && changedStem.length > 0 && relative.includes(changedStem);
      if (!colocated && !stemMatch) {
        continue;
      }
      if (relative !== changedPath && !candidates.includes(relative)) {
        candidates.push(relative);
      }
    }
  }
  return candidates.sort((a, b) => a.localeCompare(b));
}

function summarizeScene(model: GodotSceneModel | null): string {
  if (model === null) {
    return "no usable structure";
  }
  const scripts = model.nodes.filter((node) => node.script !== undefined).length;
  return `${model.nodes.length} nodes, ${scripts} script${scripts === 1 ? "" : "s"}, ${model.connections.length} connections`;
}

function summarizeResource(model: GodotResourceModel | null): string {
  if (model === null) {
    return "no usable structure";
  }
  return `${model.type}, ${model.properties.length} properties, ${model.subResources.length} subresources`;
}

/**
 * Lexical workspace containment pre-check used to classify a failed path
 * resolution: contained relative paths that fail resolution are missing
 * files; everything else is a policy violation.
 */
function isLexicallyContainedPath(path: string): boolean {
  if (path.length === 0 || path.includes("\0") || path.includes("\\")) {
    return false;
  }
  if (/^(?:[A-Za-z]:)?[\\/]/.test(path) || /^[A-Za-z]:/.test(path)) {
    return false;
  }
  const segments = path.split("/");
  return !segments.some((segment) => segment === ".." || segment === ".");
}

function classifyAutoloadTarget(path: string): "script" | "scene" | "resource" | "unknown" {
  const lower = path.toLowerCase();
  if (lower.endsWith(".gd") || lower.endsWith(".cs")) {
    return "script";
  }
  if (lower.endsWith(".tscn")) {
    return "scene";
  }
  if (lower.endsWith(".tres") || lower.endsWith(".theme")) {
    return "resource";
  }
  return "unknown";
}
