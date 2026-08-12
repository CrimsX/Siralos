import { describe, expect, it } from "vitest";
import type { GodotDiagnostics } from "@siralos/core";
import { createGodotCheckScriptTool } from "./godot-check-script-tool.js";

function diagnosticsService(
  options: {
    readonly prepareStatus?: "ready" | "unavailable" | "unsupported" | "invalid_input" | "failed";
    readonly executeStatus?: "checked" | "unavailable" | "denied" | "conflict";
  } = {},
): GodotDiagnostics {
  const prepareStatus = options.prepareStatus ?? "ready";
  const executeStatus = options.executeStatus ?? "unavailable";
  return {
    support: () =>
      Promise.resolve({ state: "unavailable", reason: "unavailable", platform: "linux" }),
    prepare: () =>
      prepareStatus === "ready"
        ? Promise.resolve({
            status: "ready",
            check: {} as never,
            preview: {
              projectName: "Fixture",
              engineVersion: "4.7.1.stable.official",
              installationId: "test-install",
              engineEdition: "standard",
              support: "compatible-untested",
              compatibility: "compatible",
              scripts: { count: 1, paths: ["src/player/player.gd"], totalBytes: 10 },
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
              manifestDigest: "a".repeat(64),
            },
            digest: "b".repeat(64),
          })
        : Promise.resolve({ status: prepareStatus, message: `${prepareStatus} reason` }),
    execute: () =>
      executeStatus === "checked"
        ? Promise.resolve({
            status: "checked",
            engineVersion: "4.7.1.stable.official",
            scriptsChecked: 1,
            validCount: 0,
            invalidCount: 1,
            diagnostics: [
              {
                source: "godot-check-only",
                severity: "error",
                path: "src/player/player.gd",
                line: 34,
                column: 17,
                code: "undeclared-identifier",
                message: 'Identifier "velocityy" not declared in the current scope.',
                rawCategory: "error",
              },
            ],
            truncated: false,
          })
        : Promise.resolve({ status: executeStatus, message: `${executeStatus} reason` }),
    status: () => ({
      state: "untrusted",
      lastResult: null,
      lastManifestDigest: null,
      lastEngineVersion: null,
    }),
    disposeAll: () => undefined,
  };
}

describe("createGodotCheckScriptTool", () => {
  it("is a reviewable prepared tool with the diagnose capability", () => {
    const tool = createGodotCheckScriptTool(diagnosticsService());
    expect(tool.kind).toBe("prepared_diagnostic");
    expect(tool.capability).toBe("godot.diagnose");
    expect(tool.definition.name).toBe("godot.check_script");
  });

  it("prepares a single-script check with a bounded preview and digest", async () => {
    const tool = createGodotCheckScriptTool(diagnosticsService());
    const prepared = await tool.prepare({ path: "src/player/player.gd" }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect(prepared.preview.scripts.paths).toEqual(["src/player/player.gd"]);
    expect(prepared.preview.isolation.checkOnly).toBe(true);
    expect(prepared.digest).toMatch(/^[0-9a-f]{64}$/);
  });

  it("rejects a missing path", async () => {
    const tool = createGodotCheckScriptTool(diagnosticsService());
    const prepared = await tool.prepare({}, {});
    expect(prepared.status).toBe("invalid_input");
  });

  it("maps unavailable and unsupported preparation truthfully", async () => {
    const tool = createGodotCheckScriptTool(diagnosticsService({ prepareStatus: "unavailable" }));
    expect((await tool.prepare({ path: "a.gd" }, {})).status).toBe("unavailable");
    const unsupported = createGodotCheckScriptTool(
      diagnosticsService({ prepareStatus: "unsupported" }),
    );
    expect((await unsupported.prepare({ path: "a.gd" }, {})).status).toBe("unsupported");
  });

  it("executes under the approved digest and returns normalized diagnostics", async () => {
    const tool = createGodotCheckScriptTool(diagnosticsService({ executeStatus: "checked" }));
    const result = await tool.executePrepared({} as never, { approvedDigest: "b".repeat(64) });
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    expect(result.output).toMatchObject({
      engineVersion: "4.7.1.stable.official",
      valid: false,
      diagnostics: [
        {
          source: "godot-check-only",
          severity: "error",
          path: "src/player/player.gd",
          line: 34,
          column: 17,
          message: 'Identifier "velocityy" not declared in the current scope.',
        },
      ],
    });
  });

  it("maps an unavailable execution to a typed unavailable tool result", async () => {
    const tool = createGodotCheckScriptTool(diagnosticsService({ executeStatus: "unavailable" }));
    const result = await tool.executePrepared({} as never, { approvedDigest: "b".repeat(64) });
    expect(result.status).toBe("unavailable");
    if (result.status === "unavailable") {
      expect(result.message).toContain("unavailable");
    }
  });

  it("maps a denied execution to denied", async () => {
    const tool = createGodotCheckScriptTool(diagnosticsService({ executeStatus: "denied" }));
    const result = await tool.executePrepared({} as never, { approvedDigest: "b".repeat(64) });
    expect(result.status).toBe("denied");
  });
});
