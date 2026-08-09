import { describe, expect, it } from "vitest";
import type {
  GodotProjectProbe,
  GodotProbePreparationResult,
  GodotRecoveryProbeResult,
  PreparedGodotProbe,
  ToolExecutionContext,
} from "@solaris/core";
import { createGodotProbeProjectTool } from "./godot-probe-project-tool.js";

function preview() {
  return {
    projectName: "Fixture",
    engineVersion: "4.7.1.stable.official",
    installationId: "test-install",
    engineEdition: "standard",
    support: "verified",
    compatibility: "compatible",
    risks: {
      toolScripts: 1,
      enabledEditorPlugins: 0,
      gdextensions: 0,
      autoloads: 0,
      dotnetProjects: 0,
    },
    mirror: { estimatedFileCount: 3, estimatedBytes: 99 },
    isolation: {
      sourceWorkspace: "not-used-as-project" as const,
      disposableMirror: true as const,
      recoveryMode: true as const,
      headless: true as const,
      network: "denied" as const,
      environment: "minimal" as const,
      stdin: "closed" as const,
    },
    manifestDigest: "m".repeat(64),
  };
}

function service(overrides: Partial<GodotProjectProbe> = {}): GodotProjectProbe {
  const probe: PreparedGodotProbe = { __brand: true } as never;
  return {
    support(): Promise<{ state: "unavailable"; reason: string; platform: string }> {
      return Promise.resolve({
        state: "unavailable",
        reason: "unavailable on this platform",
        platform: "win32",
      });
    },
    prepare(): Promise<GodotProbePreparationResult> {
      return Promise.resolve({
        status: "ready",
        probe,
        preview: preview(),
        digest: "d".repeat(64),
      });
    },
    execute(): Promise<GodotRecoveryProbeResult> {
      return Promise.resolve({
        status: "unavailable",
        engine: {
          installationId: "test-install",
          version: "4.7.1.stable.official",
          executableFingerprint: "abc",
        },
        recoveryMode: true,
        mirror: {
          sourceFiles: 0,
          sourceBytes: 0,
          generatedGodotDirectory: false,
          generatedBytes: null,
          generatedFiles: null,
          importState: "import state unknown",
        },
        diagnostics: { errors: [], warnings: [], truncated: false },
        process: { exitCode: null, durationMs: 0, timedOut: false },
        workspaceIntegrity: { unchanged: true, bounded: false },
        cleanup: { completed: true },
        message: "unavailable",
      });
    },
    status() {
      return {
        state: "untrusted",
        lastResult: null,
        lastManifestDigest: null,
        lastEngineVersion: null,
      };
    },
    disposeAll(): void {
      // no-op
    },
    ...overrides,
  };
}

describe("createGodotProbeProjectTool", () => {
  it("rejects any provider input", async () => {
    const tool = createGodotProbeProjectTool(service());
    const result = await tool.prepare({ path: "anything" }, {});
    expect(result.status).toBe("invalid_input");
  });

  it("prepares an empty input into a ready probe", async () => {
    const tool = createGodotProbeProjectTool(service());
    const result = await tool.prepare({}, {});
    expect(result.status).toBe("ready");
    if (result.status === "ready") {
      expect(result.digest).toMatch(/^[0-9a-f]{64}$/);
      expect(result.preview.isolation.recoveryMode).toBe(true);
    }
  });

  it("propagates the unavailable preparation result without requesting approval", async () => {
    const unavailable: GodotProjectProbe = {
      ...service(),
      prepare(): Promise<GodotProbePreparationResult> {
        return Promise.resolve({
          status: "unavailable",
          message: "Recovery-mode project probing is unavailable on this platform.",
        });
      },
    };
    const tool = createGodotProbeProjectTool(unavailable);
    const result = await tool.prepare({}, {});
    expect(result.status).toBe("unavailable");
    if (result.status === "unavailable") {
      expect(result.message).toContain("unavailable");
    }
  });

  it("maps an unavailable execution result to the unavailable tool status", async () => {
    const tool = createGodotProbeProjectTool(service());
    const context: ToolExecutionContext = { approvedDigest: "d".repeat(64) };
    const result = await tool.executePrepared({} as PreparedGodotProbe, context);
    expect(result.status).toBe("unavailable");
  });

  it("maps conflicts without claiming execution", async () => {
    const conflicting: GodotProjectProbe = {
      ...service(),
      execute(): Promise<GodotRecoveryProbeResult> {
        return Promise.resolve({
          status: "conflict",
          engine: { installationId: "", version: "", executableFingerprint: "" },
          recoveryMode: true,
          mirror: {
            sourceFiles: 0,
            sourceBytes: 0,
            generatedGodotDirectory: false,
            generatedBytes: null,
            generatedFiles: null,
            importState: "import state unknown",
          },
          diagnostics: { errors: [], warnings: [], truncated: false },
          process: { exitCode: null, durationMs: 0, timedOut: false },
          workspaceIntegrity: { unchanged: true, bounded: false },
          cleanup: { completed: false },
          message: "The project changed after approval.",
        });
      },
    };
    const tool = createGodotProbeProjectTool(conflicting);
    const context: ToolExecutionContext = { approvedDigest: "d".repeat(64) };
    const result = await tool.executePrepared({} as PreparedGodotProbe, context);
    expect(result.status).toBe("conflict");
  });

  it("reports cancellation during preparation", async () => {
    const cancelled: GodotProjectProbe = {
      ...service(),
      prepare(): Promise<GodotProbePreparationResult> {
        return Promise.reject(new DOMException("aborted", "AbortError"));
      },
    };
    const tool = createGodotProbeProjectTool(cancelled);
    const result = await tool.prepare({}, {});
    expect(result.status).toBe("cancelled");
  });
});
