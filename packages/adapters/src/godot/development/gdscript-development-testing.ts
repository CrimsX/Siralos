import { createHash } from "node:crypto";
import { lstat, readFile, unlink, writeFile as writeFileAsync } from "node:fs/promises";
import { join } from "node:path";
import {
  createPreparedGDScriptCheck,
  createPreparedGDScriptSession,
  type GDScriptDiagnosticResult,
  type GDScriptLanguageService,
  type GDScriptLanguageSession,
  type GDScriptLSPSessionPreview,
  type GDScriptQueryOutcome,
  type GitDiffResult,
  type GitInspector,
  type GitStatusRequest,
  type GitStatusResult,
  type GitWorkspaceStatus,
  type GodotDiagnostics,
  type GodotGDScriptDiagnostic,
  type GodotProjectCheckResult,
  type PreparedGDScriptCheck,
  type PreparedGDScriptSession,
} from "@solaris/core";

/**
 * Test doubles for the GDScript development workflow: a scripted language
 * service (with a controllable fake session), a scripted parser service,
 * a real-filesystem change-set primitives implementation, and a
 * recording Git inspector. Only the workflow's port dependencies are
 * faked; the checkpoint store and the workspace filesystem are real.
 */

export interface FakeSessionControl {
  readonly session: GDScriptLanguageSession;
  diagnosticsByPath: Map<string, readonly GodotGDScriptDiagnostic[]>;
  closed: boolean;
  openDocuments: string[];
}

export interface FakeLanguageControl {
  engine: {
    readonly sha256: string;
    readonly version: string;
    readonly installationId: string;
  } | null;
  active: FakeSessionControl | null;
  prepareCount: number;
  startCount: number;
  closeAllCount: number;
  /** Fail the next session start with this message (infrastructure failure). */
  nextStartFailure: string | null;
  /** Fail closeAll with this error. */
  closeAllError: Error | null;
  /** Diagnostics to seed into the NEXT started session, by path. */
  nextSessionDiagnostics: Map<string, readonly GodotGDScriptDiagnostic[]>;
  log: string[];
}

export interface FakeLanguageOptions {
  readonly engine?: {
    readonly sha256: string;
    readonly version: string;
    readonly installationId: string;
  };
}

export function createFakeLanguageService(options: FakeLanguageOptions = {}): {
  readonly service: GDScriptLanguageService;
  readonly control: FakeLanguageControl;
} {
  const control: FakeLanguageControl = {
    engine: options.engine ?? {
      sha256: "e".repeat(64),
      version: "4.7.1-stable",
      installationId: "test-installation",
    },
    active: null,
    prepareCount: 0,
    startCount: 0,
    closeAllCount: 0,
    nextStartFailure: null,
    closeAllError: null,
    nextSessionDiagnostics: new Map(),
    log: [],
  };
  const service: GDScriptLanguageService = {
    support: () => Promise.resolve({ state: "available", reason: null, platform: "test" }),
    activeSession: () => control.active?.session ?? null,
    selectedEngine: () => Promise.resolve(control.engine),
    prepare: () => {
      control.prepareCount += 1;
      control.log.push("prepare");
      const digest = createHash("sha256")
        .update(`plan-${control.prepareCount}-${control.engine?.sha256 ?? "none"}`)
        .digest("hex");
      const preview: GDScriptLSPSessionPreview = {
        projectName: "fixture",
        engineVersion: control.engine?.version ?? "unknown",
        installationId: control.engine?.installationId ?? "test",
        engineEdition: "standard",
        support: "verified",
        compatibility: "compatible",
        projectIntelligence: {
          gdscriptFiles: 1,
          toolScripts: 0,
          editorPlugins: 0,
          gdextensions: 0,
        },
        session: {
          sourceProject: "disposable mirror",
          godotMode: "headless recovery editor",
          lspNetwork: "loopback only",
          externalNetwork: "denied",
          sourceWrites: "denied",
          providerSecrets: "removed",
          lspMutations: "disabled",
        },
        capabilities: { diagnostics: true, hover: true, completion: true, definition: true },
        manifestDigest: "m".repeat(64),
      };
      return Promise.resolve({
        status: "ready",
        session: createPreparedGDScriptSession(),
        preview,
        digest,
      });
    },
    start: (_session: PreparedGDScriptSession, context) => {
      control.startCount += 1;
      control.log.push("start");
      if (context.approvedDigest === "") {
        return Promise.resolve({ status: "conflict", message: "no approved digest" });
      }
      if (control.nextStartFailure !== null) {
        const message = control.nextStartFailure;
        control.nextStartFailure = null;
        return Promise.resolve({ status: "failed", message });
      }
      const sessionId = `lsp-${control.startCount}`;
      const sessionControl: FakeSessionControl = {
        session: createFakeSession(sessionId, control),
        diagnosticsByPath: new Map(control.nextSessionDiagnostics),
        closed: false,
        openDocuments: [],
      };
      control.nextSessionDiagnostics = new Map();
      control.active = sessionControl;
      control.log.push(`started-${sessionId}`);
      return Promise.resolve({ status: "ready", session: sessionControl.session });
    },
    status: () => ({
      state: control.active === null ? ("unavailable" as const) : ("ready" as const),
      sessionId: control.active === null ? null : control.active.session.id,
      engineVersion: control.engine?.version ?? null,
      projectName: "fixture",
      startedAtMs: null,
      idleMs: null,
      capabilities: { diagnostics: true, hover: true, completion: true, definition: true },
      openDocumentCount: control.active?.openDocuments.length ?? 0,
      diagnosticCount: 0,
      networkIsolation: "unavailable" as const,
    }),
    closeAll: () => {
      control.closeAllCount += 1;
      control.log.push("closeAll");
      if (control.closeAllError !== null) {
        throw control.closeAllError;
      }
      if (control.active !== null) {
        control.active.closed = true;
        control.active = null;
      }
      return Promise.resolve();
    },
  };
  return { service, control };
}

function createFakeSession(id: string, control: FakeLanguageControl): GDScriptLanguageSession {
  return {
    id,
    engineVersion: control.engine?.version ?? "unknown",
    getStatus: () => ({
      state: "ready" as const,
      sessionId: id,
      engineVersion: control.engine?.version ?? null,
      projectName: "fixture",
      startedAtMs: null,
      idleMs: null,
      capabilities: { diagnostics: true, hover: true, completion: true, definition: true },
      openDocumentCount: 0,
      diagnosticCount: 0,
      networkIsolation: "unavailable" as const,
    }),
    openDocument: (request) => {
      const sessionControl = control.active;
      if (sessionControl === null || sessionControl.closed) {
        return Promise.resolve({ status: "session_required", message: "no session" });
      }
      sessionControl.openDocuments.push(request.path);
      return Promise.resolve({ status: "ready", result: undefined });
    },
    closeDocument: () => Promise.resolve({ status: "ready", result: undefined }),
    hover: () =>
      Promise.resolve({ status: "ready", result: { path: "x.gd", range: null, contents: [] } }),
    completion: () =>
      Promise.resolve({ status: "ready", result: { path: "x.gd", items: [], truncated: false } }),
    definition: () =>
      Promise.resolve({
        status: "ready",
        result: { path: "x.gd", locations: [], truncated: false },
      }),
    diagnostics: (request): Promise<GDScriptQueryOutcome<GDScriptDiagnosticResult>> => {
      const sessionControl = control.active;
      if (sessionControl === null || sessionControl.closed) {
        return Promise.resolve({ status: "session_required", message: "no session" });
      }
      const diagnostics = sessionControl.diagnosticsByPath.get(request.path) ?? [];
      return Promise.resolve({
        status: "ready",
        result: { path: request.path, diagnostics: [...diagnostics], truncated: false },
      });
    },
    close: () => {
      const sessionControl = control.active;
      if (sessionControl !== null) {
        sessionControl.closed = true;
        control.active = null;
      }
      return Promise.resolve();
    },
  };
}

export interface FakeParserControl {
  /** path -> outcome for the next execute; absent = clean. */
  resultsByPath: Map<
    string,
    { readonly valid: boolean; readonly diagnostics: readonly GodotGDScriptDiagnostic[] }
  >;
  /** FIFO queue of scripted outcomes consumed per execute; falls back to resultsByPath. */
  queuedResults: {
    readonly path: string;
    readonly valid: boolean;
    readonly diagnostics: readonly GodotGDScriptDiagnostic[];
  }[];
  /** Fail the next prepare with this message (infrastructure failure). */
  nextPrepareFailure: string | null;
  /** Fail the next execute with this message. */
  nextExecuteFailure: string | null;
  log: string[];
  /** Paths of the most recent prepared request. */
  lastPaths: readonly string[];
}

export function createFakeDiagnosticsService(): {
  readonly service: GodotDiagnostics;
  readonly control: FakeParserControl;
} {
  const control: FakeParserControl = {
    resultsByPath: new Map(),
    queuedResults: [],
    nextPrepareFailure: null,
    nextExecuteFailure: null,
    log: [],
    lastPaths: [],
  };
  const service: GodotDiagnostics = {
    support: () => Promise.resolve({ state: "available", reason: null, platform: "test" }),
    prepare: async (request) => {
      control.log.push("prepare");
      control.lastPaths = request.paths ?? [];
      if (control.nextPrepareFailure !== null) {
        const message = control.nextPrepareFailure;
        control.nextPrepareFailure = null;
        return Promise.resolve({ status: "unavailable", message });
      }
      const digest = createHash("sha256")
        .update(JSON.stringify(request.paths ?? []))
        .digest("hex");
      return Promise.resolve({
        status: "ready",
        check: createPreparedGDScriptCheck(),
        preview: {
          projectName: "fixture",
          engineVersion: "4.7.1-stable",
          installationId: "test",
          engineEdition: "standard",
          support: "verified",
          compatibility: "compatible",
          scripts: {
            count: request.paths?.length ?? 0,
            paths: request.paths ?? null,
            totalBytes: 0,
          },
          operation: "parse-only",
          isolation: {
            sourceWorkspace: "not-used-as-project",
            disposableMirror: true,
            checkOnly: true,
            headless: true,
            sceneExecution: "disabled",
            gameExecution: "disabled",
            network: "denied",
            environment: "minimal",
            stdin: "closed",
          },
          manifestDigest: "m".repeat(64),
        },
        digest,
      });
    },
    execute: (check: PreparedGDScriptCheck, context) => {
      control.log.push("execute");
      if (context.approvedDigest === "") {
        return Promise.resolve({ status: "conflict", message: "no approved digest" });
      }
      if (control.nextExecuteFailure !== null) {
        const message = control.nextExecuteFailure;
        control.nextExecuteFailure = null;
        return Promise.resolve({ status: "failed", message });
      }
      let validCount = 0;
      const diagnostics: GodotGDScriptDiagnostic[] = [];
      for (const path of control.lastPaths) {
        const scripted = control.queuedResults.shift() ?? control.resultsByPath.get(path);
        if (scripted === undefined || scripted.valid) {
          validCount += 1;
        }
        if (scripted !== undefined) {
          diagnostics.push(...scripted.diagnostics);
        }
      }
      const result: GodotProjectCheckResult = {
        status: "checked",
        engineVersion: "4.7.1-stable",
        scriptsChecked: control.lastPaths.length,
        validCount,
        invalidCount: control.lastPaths.length - validCount,
        diagnostics,
        truncated: false,
      };
      void check;
      return Promise.resolve(result);
    },
    status: () => ({
      state: "untrusted",
      lastResult: null,
      lastManifestDigest: null,
      lastEngineVersion: null,
    }),
    disposeAll: () => undefined,
  };
  return { service, control };
}

/** Real-filesystem change-set primitives for the workflow tests. */
export function createWorkspaceFilePrimitives(workspaceRoot: string) {
  return {
    async readFile(
      path: string,
    ): Promise<{ readonly exists: boolean; readonly sha256: string | null }> {
      const absolute = join(workspaceRoot, path);
      try {
        const stats = await lstat(absolute);
        if (!stats.isFile()) {
          return { exists: false, sha256: null };
        }
        const bytes = await readFile(absolute);
        return { exists: true, sha256: sha256OfBytes(bytes) };
      } catch {
        return { exists: false, sha256: null };
      }
    },
    async readContent(path: string): Promise<{
      readonly exists: boolean;
      readonly sha256: string | null;
      readonly content: string | null;
    }> {
      const absolute = join(workspaceRoot, path);
      try {
        const stats = await lstat(absolute);
        if (!stats.isFile()) {
          return { exists: false, sha256: null, content: null };
        }
        const bytes = await readFile(absolute);
        return { exists: true, sha256: sha256OfBytes(bytes), content: bytes.toString("utf8") };
      } catch {
        return { exists: false, sha256: null, content: null };
      }
    },
    async writeFile(path: string, content: string): Promise<void> {
      const absolute = join(workspaceRoot, path);
      const { mkdir } = await import("node:fs/promises");
      await mkdir(join(absolute, ".."), { recursive: true });
      await writeFileAsync(absolute, content, "utf8");
    },
    async deleteFile(path: string): Promise<void> {
      await unlink(join(workspaceRoot, path));
    },
  };
}

export function createFakeGitInspector(): {
  readonly git: GitInspector;
  readonly statusResult: GitStatusResult;
  readonly log: string[];
} {
  const log: string[] = [];
  const statusResult: GitStatusResult = {
    repository: true,
    branch: {
      head: "main",
      oid: "abc123",
      upstream: null,
      ahead: null,
      behind: null,
      detached: false,
      unborn: false,
    },
    changes: [
      {
        path: "src/player/player.gd",
        originalPath: null,
        indexStatus: "unmodified",
        worktreeStatus: "modified",
        kind: "ordinary",
      },
    ],
    conflicts: [],
    untracked: [],
    truncated: false,
  };
  const git: GitInspector = {
    inspectRepository: (): Promise<GitWorkspaceStatus> => {
      log.push("inspectRepository");
      return Promise.resolve({
        gitAvailable: true,
        gitVersion: "2.40",
        repositoryState: "repository",
        repositoryRoot: null,
      });
    },
    getStatus: (request: GitStatusRequest): Promise<GitStatusResult> => {
      log.push("getStatus");
      void request;
      return Promise.resolve(statusResult);
    },
    getDiff: (): Promise<GitDiffResult> => {
      log.push("getDiff");
      return Promise.resolve({
        scope: "working",
        files: [],
        patch: "",
        truncated: false,
        untrackedExcluded: true,
      });
    },
  };
  return { git, statusResult, log };
}

export function sha256OfBytes(bytes: Uint8Array): string {
  return createHash("sha256").update(bytes).digest("hex");
}

export function sha256Of(text: string): string {
  return sha256OfBytes(new TextEncoder().encode(text));
}
