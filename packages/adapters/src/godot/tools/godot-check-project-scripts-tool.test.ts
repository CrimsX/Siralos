import { describe, expect, it } from "vitest";
import type { GodotDiagnostics } from "@siralos/core";
import { createGodotCheckProjectScriptsTool } from "./godot-check-project-scripts-tool.js";

function diagnosticsService(
  options: {
    readonly prepareStatus?: "ready" | "unavailable" | "unsupported" | "invalid_input" | "failed";
    readonly executeStatus?: "checked" | "unavailable" | "denied" | "conflict";
    readonly scriptCount?: number;
  } = {},
): GodotDiagnostics {
  const prepareStatus = options.prepareStatus ?? "ready";
  const executeStatus = options.executeStatus ?? "unavailable";
  const scriptCount = options.scriptCount ?? 2;
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
              scripts: { count: scriptCount, paths: null, totalBytes: 100 },
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
            scriptsChecked: scriptCount,
            validCount: 1,
            invalidCount: 1,
            diagnostics: [
              {
                source: "godot-check-only",
                severity: "error",
                path: "src/ui/menu.gd",
                line: 81,
                column: 9,
                code: "parse-error",
                message: 'Expected "end of file" after class body.',
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

describe("createGodotCheckProjectScriptsTool", () => {
  it("is a reviewable prepared tool with the diagnose capability", () => {
    const tool = createGodotCheckProjectScriptsTool(diagnosticsService());
    expect(tool.kind).toBe("prepared_diagnostic");
    expect(tool.capability).toBe("godot.diagnose");
    expect(tool.definition.name).toBe("godot.check_project_scripts");
  });

  it("prepares a project-wide check with the aggregated script count", async () => {
    const tool = createGodotCheckProjectScriptsTool(diagnosticsService({ scriptCount: 3 }));
    const prepared = await tool.prepare({}, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect(prepared.preview.scripts.count).toBe(3);
    expect(prepared.preview.scripts.paths).toBeNull();
  });

  it("passes an explicit bounded paths subset through", async () => {
    const tool = createGodotCheckProjectScriptsTool(diagnosticsService());
    const prepared = await tool.prepare({ paths: ["src/ui/menu.gd"] }, {});
    expect(prepared.status).toBe("ready");
  });

  it("rejects malformed subsets and non-array input", async () => {
    const tool = createGodotCheckProjectScriptsTool(diagnosticsService());
    expect((await tool.prepare({ paths: [1] }, {})).status).toBe("invalid_input");
    expect((await tool.prepare({ paths: "a.gd" }, {})).status).toBe("invalid_input");
  });

  it("aggregates normalized diagnostics on a checked run", async () => {
    const tool = createGodotCheckProjectScriptsTool(
      diagnosticsService({ executeStatus: "checked" }),
    );
    const result = await tool.executePrepared({} as never, { approvedDigest: "b".repeat(64) });
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    expect(result.output).toMatchObject({
      engineVersion: "4.7.1.stable.official",
      scriptsChecked: 2,
      validCount: 1,
      invalidCount: 1,
      diagnostics: [{ severity: "error", path: "src/ui/menu.gd", line: 81, column: 9 }],
    });
  });

  it("maps unavailable execution truthfully", async () => {
    const tool = createGodotCheckProjectScriptsTool(
      diagnosticsService({ executeStatus: "unavailable" }),
    );
    const result = await tool.executePrepared({} as never, { approvedDigest: "b".repeat(64) });
    expect(result.status).toBe("unavailable");
  });

  it("maps a conflict to conflict", async () => {
    const tool = createGodotCheckProjectScriptsTool(
      diagnosticsService({ executeStatus: "conflict" }),
    );
    const result = await tool.executePrepared({} as never, { approvedDigest: "b".repeat(64) });
    expect(result.status).toBe("conflict");
  });
});
