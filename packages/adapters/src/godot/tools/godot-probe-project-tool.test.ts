import { describe, expect, it } from "vitest";
import type {
  GodotProbePreview,
  GodotProjectProbe,
  GodotProbePreparationResult,
  GodotRecoveryProbeResult,
  PreparedGodotProbe,
} from "@solaris/core";
import { createPreparedGodotProbe } from "@solaris/core";
import { createGodotProbeProjectTool } from "./godot-probe-project-tool.js";

function samplePreview(): GodotProbePreview {
  return {
    projectName: "Fixture",
    engineVersion: "4.7.1.stable.official",
    installationId: "path-1",
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
    mirror: { estimatedFileCount: 3, estimatedBytes: 42 },
    isolation: {
      sourceWorkspace: "not-used-as-project",
      disposableMirror: true,
      recoveryMode: true,
      headless: true,
      network: "denied",
      environment: "minimal",
      stdin: "closed",
    },
    manifestDigest: "m".repeat(64),
  };
}

function sampleResult(overrides: Partial<GodotRecoveryProbeResult> = {}): GodotRecoveryProbeResult {
  return {
    status: "completed",
    engine: {
      installationId: "path-1",
      version: "4.7.1.stable.official",
      executableFingerprint: "abc123",
    },
    recoveryMode: true,
    mirror: {
      sourceFiles: 3,
      sourceBytes: 42,
      generatedGodotDirectory: true,
      generatedBytes: 1024,
      generatedFiles: 2,
      importState: "imports observed",
    },
    diagnostics: { errors: [], warnings: [], truncated: false },
    process: { exitCode: 0, durationMs: 1500, timedOut: false },
    workspaceIntegrity: { unchanged: true, bounded: false },
    cleanup: { completed: true },
    message: "completed",
    ...overrides,
  };
}

function createStubProbe(
  options: {
    prepareResult?: GodotProbePreparationResult;
    executionResult?: GodotRecoveryProbeResult;
    executionError?: Error;
  } = {},
): {
  probe: GodotProjectProbe;
  executedDigests: () => string[];
} {
  const executedDigests: string[] = [];
  return {
    probe: {
      prepare(): Promise<GodotProbePreparationResult> {
        return Promise.resolve(
          options.prepareResult ?? {
            status: "ready",
            probe: createPreparedGodotProbe(),
            preview: samplePreview(),
            digest: "prepared-digest",
          },
        );
      },
      execute(
        _prepared: PreparedGodotProbe,
        context: { readonly approvedDigest: string; readonly signal?: AbortSignal },
      ): Promise<GodotRecoveryProbeResult> {
        executedDigests.push(context.approvedDigest);
        if (options.executionError !== undefined) {
          return Promise.reject(options.executionError);
        }
        return Promise.resolve(options.executionResult ?? sampleResult());
      },
      status() {
        return {
          state: "untrusted" as const,
          lastResult: null,
          lastManifestDigest: null,
          lastEngineVersion: null,
        };
      },
    },
    executedDigests: () => [...executedDigests],
  };
}

describe("godot.probe_project tool", () => {
  it("accepts only an empty input object", async () => {
    const { probe } = createStubProbe();
    const tool = createGodotProbeProjectTool(probe);
    const accepted = await tool.prepare({}, {});
    expect(accepted.status).toBe("ready");
    const rejected = await tool.prepare({ executable: "/opt/godot" }, {});
    expect(rejected.status).toBe("invalid_input");
  });

  it("forwards the approved digest to execution", async () => {
    const { probe, executedDigests } = createStubProbe();
    const tool = createGodotProbeProjectTool(probe);
    const prepared = await tool.prepare({}, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.executePrepared(prepared.probe, {
      approvedDigest: "prepared-digest",
    });
    expect(result.status).toBe("success");
    expect(executedDigests()).toEqual(["prepared-digest"]);
  });

  it("never exposes mirror or source absolute paths in the provider result", async () => {
    const { probe } = createStubProbe();
    const tool = createGodotProbeProjectTool(probe);
    const prepared = await tool.prepare({}, {});
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.executePrepared(prepared.probe, { approvedDigest: prepared.digest });
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    const serialized = JSON.stringify(result.output);
    expect(serialized).not.toContain("C:");
    expect(serialized).not.toContain("/runs/");
    expect(serialized).not.toContain("/workspace");
    expect(serialized).not.toContain("projectPath");
    const record = result.output as Record<string, unknown>;
    expect(record["recoveryMode"]).toBe(true);
    expect(record["sourceWorkspaceLoaded"]).toBe(false);
  });

  it("states recovery mode and source-workspace non-loading in the result", async () => {
    const { probe } = createStubProbe();
    const tool = createGodotProbeProjectTool(probe);
    const prepared = await tool.prepare({}, {});
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.executePrepared(prepared.probe, { approvedDigest: prepared.digest });
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    const record = result.output as Record<string, unknown>;
    expect(record["recoveryMode"]).toBe(true);
    expect(record["sourceWorkspaceLoaded"]).toBe(false);
  });

  it("maps conflicts, timeouts, and cancellations truthfully", async () => {
    const conflict = createStubProbe({
      executionResult: sampleResult({ status: "conflict" }),
    });
    const timedOut = createStubProbe({
      executionResult: sampleResult({ status: "timed_out" }),
    });
    const cancelled = createStubProbe({
      executionResult: sampleResult({ status: "cancelled" }),
    });
    for (const [stub, expected] of [
      [conflict, "conflict"],
      [timedOut, "timed_out"],
      [cancelled, "cancelled"],
    ] as const) {
      const tool = createGodotProbeProjectTool(stub.probe);
      const prepared = await tool.prepare({}, {});
      if (prepared.status !== "ready") {
        return;
      }
      const result = await tool.executePrepared(prepared.probe, {
        approvedDigest: prepared.digest,
      });
      expect(result.status).toBe(expected);
    }
  });

  it("maps sandbox failures and workspace violations", async () => {
    const sandboxFailed = createStubProbe({
      executionResult: sampleResult({ status: "sandbox_failed" }),
    });
    const workspaceChanged = createStubProbe({
      executionResult: sampleResult({ status: "workspace_changed" }),
    });
    const tool1 = createGodotProbeProjectTool(sandboxFailed.probe);
    const prepared1 = await tool1.prepare({}, {});
    if (prepared1.status !== "ready") {
      return;
    }
    expect(
      (await tool1.executePrepared(prepared1.probe, { approvedDigest: prepared1.digest })).status,
    ).toBe("sandbox_unavailable");
    const tool2 = createGodotProbeProjectTool(workspaceChanged.probe);
    const prepared2 = await tool2.prepare({}, {});
    if (prepared2.status !== "ready") {
      return;
    }
    expect(
      (await tool2.executePrepared(prepared2.probe, { approvedDigest: prepared2.digest })).status,
    ).toBe("workspace_violation");
  });

  it("propagates prepare failures", async () => {
    const { probe } = createStubProbe({
      prepareResult: { status: "failed", message: "No trusted editor selected." },
    });
    const tool = createGodotProbeProjectTool(probe);
    const prepared = await tool.prepare({}, {});
    expect(prepared.status).toBe("failed");
    if (prepared.status === "failed") {
      expect(prepared.message).toContain("No trusted editor");
    }
  });
});
