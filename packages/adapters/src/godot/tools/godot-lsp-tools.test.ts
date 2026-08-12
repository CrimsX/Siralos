import { describe, expect, it } from "vitest";
import type { GDScriptLanguageService } from "@siralos/core";
import { createGodotLSPSessionTool } from "./godot-lsp-session-tool.js";
import {
  createGodotCompleteTool,
  createGodotDefinitionTool,
  createGodotHoverTool,
  createGodotLSPDiagnosticsTool,
} from "./godot-lsp-query-tools.js";

function unavailableService(): GDScriptLanguageService {
  return {
    support: () =>
      Promise.resolve({ state: "unavailable", reason: "unavailable", platform: "linux" }),
    activeSession: () => null,
    selectedEngine: () => Promise.resolve(null),
    prepare: () => Promise.resolve({ status: "unavailable", message: "unavailable" }),
    start: () => Promise.resolve({ status: "unavailable", message: "unavailable" }),
    status: () => ({
      state: "unavailable" as const,
      sessionId: null,
      engineVersion: null,
      projectName: null,
      startedAtMs: null,
      idleMs: null,
      capabilities: { diagnostics: false, hover: false, completion: false, definition: false },
      openDocumentCount: 0,
      diagnosticCount: 0,
      networkIsolation: "unavailable" as const,
    }),
    closeAll: () => Promise.resolve(),
  };
}

describe("createGodotLSPSessionTool", () => {
  it("is a reviewable prepared tool with the godot.lsp capability", () => {
    const tool = createGodotLSPSessionTool(unavailableService());
    expect(tool.kind).toBe("prepared_lsp_session");
    expect(tool.capability).toBe("godot.lsp");
    expect(tool.definition.name).toBe("godot.lsp_session");
  });

  it("reports unavailable preparation without requesting approval", async () => {
    const tool = createGodotLSPSessionTool(unavailableService());
    const prepared = await tool.prepare({}, {});
    expect(prepared.status).toBe("unavailable");
  });

  it("accepts no input", async () => {
    const tool = createGodotLSPSessionTool(unavailableService());
    const prepared = await tool.prepare({ anything: true }, {});
    expect(prepared.status).toBe("invalid_input");
  });

  it("defers to an active development workflow's session ownership", async () => {
    const tool = createGodotLSPSessionTool(unavailableService(), () => ({
      blocked: true,
      message: "the development workflow manages the language session lifecycle",
    }));
    const prepared = await tool.prepare({}, {});
    expect(prepared.status).toBe("failed");
    if (prepared.status === "failed") {
      expect(prepared.message).toContain("development workflow manages");
    }
  });
});

describe("GDScript language query tools", () => {
  const tools = [
    createGodotHoverTool(unavailableService()),
    createGodotCompleteTool(unavailableService()),
    createGodotDefinitionTool(unavailableService()),
    createGodotLSPDiagnosticsTool(unavailableService()),
  ];

  it("requires an active session with a typed failure", async () => {
    for (const tool of tools) {
      const input =
        tool.definition.name === "godot.lsp_diagnostics"
          ? { path: "src/player/player.gd" }
          : { path: "src/player/player.gd", line: 10, column: 5 };
      const result = await tool.execute(input, {});
      expect(result.status).toBe("failed");
      if (result.status === "failed") {
        expect(result.message).toContain("No Godot language session is active");
      }
    }
  });

  it("validates input strictly", async () => {
    const hover = createGodotHoverTool(unavailableService());
    expect((await hover.execute({}, {})).status).toBe("invalid_input");
    expect((await hover.execute({ path: "a.gd", line: 0, column: 1 }, {})).status).toBe(
      "invalid_input",
    );
    expect((await hover.execute({ path: "a.gd", line: 1.5, column: 1 }, {})).status).toBe(
      "invalid_input",
    );
  });

  it("exposes no host or port input to the provider", () => {
    for (const tool of tools) {
      const schema = JSON.stringify(tool.definition.inputSchema);
      expect(schema).not.toContain("host");
      expect(schema).not.toContain("port");
      expect(schema).not.toContain("method");
    }
  });
});
