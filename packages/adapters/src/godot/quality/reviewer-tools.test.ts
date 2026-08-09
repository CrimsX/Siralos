import { describe, expect, it } from "vitest";
import { createFakeSandboxBackend, createRunDirectoryProvider } from "@solaris/adapters";
import {
  createGDScriptDevelopmentService,
  createGodotInspector,
  createGDScriptLanguageService,
  createGodotDiagnosticsService,
  createGodotKnowledgeService,
  createGodotKnowledgeCache,
  createGodotProbeRunner,
  createGodotProjectProbeService,
  createEngineProfileCache,
  createGitCliAdapter,
  createMutationLock,
  createFilesystemCheckpointStore,
  DEFAULT_CHECKPOINT_ROOT,
} from "@solaris/adapters";
import {
  type GDScriptLanguageService,
  type GitInspector,
  type GodotInspector,
  type GodotKnowledge,
  type ToolRegistry,
} from "@solaris/core";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createReviewerToolRegistry } from "./reviewer-tools.js";

/**
 * Reviewer tool-registry tests (ADR 0013 §27, §88–§98). The independent
 * reviewer may inspect but never mutate: the registry contains no write
 * tools, no process execution, no approval controls, no checkpoint
 * mutation, and no undo.
 */

const WRITE_TOOL_NAMES = [
  "workspace.create_file",
  "workspace.edit_file",
  "workspace.delete_file",
  "workspace.apply_text_changeset",
  "process.run",
  "godot.check_script",
  "godot.check_project_scripts",
  "godot.probe_project",
  "godot.lsp_session",
  "godot.complete",
];

async function buildReviewerRegistry(): Promise<{
  readonly registry: ToolRegistry;
  readonly cleanup: () => Promise<void>;
}> {
  const parent = await mkdtemp(join(tmpdir(), "solaris-reviewer-tools-"));
  const root = join(parent, "workspace");
  await import("node:fs/promises").then((fs) => fs.mkdir(root, { recursive: true }));
  await writeFile(join(root, "project.godot"), '[application]\nconfig/name="fixture"\n');
  const backend = createFakeSandboxBackend({}).backend;
  const runDirectories = createRunDirectoryProvider({
    workspaceRoot: root,
    runsRoot: join(parent, "runs"),
  });
  const git: GitInspector = createGitCliAdapter({
    workspaceRoot: root,
    backend,
    runDirectories,
  });
  const cache = createEngineProfileCache({ rootDirectory: join(parent, "cache") });
  const knowledgeCache = createGodotKnowledgeCache({
    rootDirectory: join(parent, "knowledge-cache"),
  });
  const config = { activeInstallation: null, installations: {}, discoverOnPath: false };
  const parentEnvironment = { PATH: "", PATHEXT: "" };
  const probeRunner = createGodotProbeRunner({ backend, runDirectories, parentEnvironment });
  const preference = { kind: "none" } as const;
  const recoveryProbe = createGodotProjectProbeService({
    workspaceRoot: root,
    config,
    preference,
    overrideSource: null,
    backend,
    probeRunner,
    cache,
    hostPath: null,
    hostPathExt: null,
    platform: "linux",
    runDirectories,
    checkpointRoot: DEFAULT_CHECKPOINT_ROOT,
    git,
    parentEnvironment,
  });
  const godot: GodotInspector = createGodotInspector({
    config,
    preference,
    overrideSource: null,
    workspaceRoot: root,
    backend,
    probeRunner,
    cache,
    hostPath: null,
    hostPathExt: null,
    platform: "linux",
    recoveryProbe,
  });
  const knowledge: GodotKnowledge = createGodotKnowledgeService({
    workspaceRoot: root,
    config,
    preference,
    overrideSource: null,
    backend,
    probeRunner,
    cache: knowledgeCache,
    engineProfileCache: cache,
    hostPath: null,
    hostPathExt: null,
    platform: "linux",
    parentEnvironment,
  });
  const checkpoints = await createFilesystemCheckpointStore({
    workspaceRoot: root,
    rootDirectory: join(parent, "checkpoints"),
  });
  const language: GDScriptLanguageService = createGDScriptLanguageService({
    workspaceRoot: root,
    config,
    preference,
    overrideSource: null,
    backend,
    probeRunner,
    cache,
    hostPath: null,
    hostPathExt: null,
    platform: "linux",
    runDirectories,
    checkpointRoot: DEFAULT_CHECKPOINT_ROOT,
    parentEnvironment,
  });
  const development = createGDScriptDevelopmentService({
    workspaceRoot: root,
    platform: "linux",
    store: checkpoints,
    lock: createMutationLock(),
    language,
    diagnostics: createGodotDiagnosticsService({
      workspaceRoot: root,
      config,
      preference,
      overrideSource: null,
      backend,
      probeRunner,
      cache,
      hostPath: null,
      hostPathExt: null,
      platform: "linux",
      runDirectories,
      checkpointRoot: DEFAULT_CHECKPOINT_ROOT,
      parentEnvironment,
    }),
    git,
    canApplyIdentityBound: false,
    primitives: {
      readFile: () => Promise.resolve({ exists: false, sha256: null }),
      readContent: () => Promise.resolve({ exists: false, sha256: null, content: null }),
      writeFile: () => Promise.resolve(),
      deleteFile: () => Promise.resolve(),
    },
  });
  return {
    registry: createReviewerToolRegistry({
      workspaceRoot: root,
      git,
      godot,
      knowledge,
      language,
      languageQueryGate: () => development.languageQueryGate(),
    }),
    cleanup: async () => {
      await rm(parent, { recursive: true, force: true });
    },
  };
}

describe("reviewer tool registry", () => {
  it("contains only read-only inspection tools", async () => {
    const { registry, cleanup } = await buildReviewerRegistry();
    try {
      const names = registry.definitions().map((info) => info.definition.name);
      expect(names).toContain("workspace.list");
      expect(names).toContain("workspace.read");
      expect(names).toContain("workspace.search");
      expect(names).toContain("git.status");
      expect(names).toContain("git.diff");
      expect(names).toContain("godot.inspect_project");
      expect(names).toContain("godot.api_search");
      expect(names).toContain("godot.api_lookup");
      expect(names).toContain("godot.hover");
      expect(names).toContain("godot.definition");
      expect(names).toContain("godot.lsp_diagnostics");
      for (const forbidden of WRITE_TOOL_NAMES) {
        expect(names).not.toContain(forbidden);
      }
    } finally {
      await cleanup();
    }
  });

  it("grants no capability outside the read-only set", async () => {
    const { registry, cleanup } = await buildReviewerRegistry();
    try {
      for (const info of registry.definitions()) {
        expect([
          "workspace.read",
          "git.inspect",
          "godot.inspect",
          "godot.api",
          "godot.lsp",
        ]).toContain(info.capability);
      }
    } finally {
      await cleanup();
    }
  });

  it("never exposes mutation, process, or approval tools to the reviewer", async () => {
    const { registry, cleanup } = await buildReviewerRegistry();
    try {
      expect(registry.get("workspace.read")).toBeDefined();
      expect(registry.get("workspace.list")).toBeDefined();
      expect(registry.get("workspace.apply_text_changeset")).toBeUndefined();
      expect(registry.get("process.run")).toBeUndefined();
      expect(registry.get("godot.check_script")).toBeUndefined();
    } finally {
      await cleanup();
    }
  });
});
