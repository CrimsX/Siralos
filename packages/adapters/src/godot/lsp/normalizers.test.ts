import { describe, expect, it } from "vitest";
import { GODOT_LIMITS } from "@solaris/core";
import {
  normalizeCompletion,
  normalizeDefinition,
  normalizeHover,
  normalizePublishDiagnostics,
} from "./normalizers.js";
import { pathToFileUri } from "./file-uri.js";

const MIRROR = process.platform === "win32" ? "C:\\solaris\\mirror-1" : "/tmp/solaris/mirror-1";

function uri(relativePath: string): string {
  return pathToFileUri(`${MIRROR}/${relativePath}`);
}

function context(relativePath: string) {
  return { mirrorRootPath: MIRROR, path: relativePath };
}

describe("normalizePublishDiagnostics", () => {
  it("maps errors, warnings, and informational diagnostics with 1-based positions", () => {
    const result = normalizePublishDiagnostics(
      uri("src/player/player.gd"),
      [
        {
          range: { start: { line: 33, character: 16 }, end: { line: 33, character: 27 } },
          severity: 1,
          code: "undeclared_identifier",
          message: 'Identifier "velocityy" not declared in the current scope.',
          source: "gdscript",
        },
        {
          range: { start: { line: 4, character: 0 }, end: { line: 4, character: 4 } },
          severity: 2,
          message: "Unused variable.",
        },
        {
          range: { start: { line: 9, character: 0 }, end: { line: 9, character: 1 } },
          severity: 3,
          message: "Info note.",
        },
        {
          range: { start: { line: 1, character: 0 }, end: { line: 1, character: 1 } },
          severity: 7,
          message: "Unknown severity preserved.",
        },
      ],
      context("src/player/player.gd"),
    );
    expect(result?.path).toBe("src/player/player.gd");
    expect(result?.diagnostics).toHaveLength(4);
    const [error, warning, info, unknown] = result?.diagnostics ?? [];
    expect(error).toMatchObject({
      source: "godot-lsp",
      severity: "error",
      line: 34,
      column: 17,
      code: "undeclared_identifier",
      rawCategory: "gdscript",
    });
    expect(warning?.severity).toBe("warning");
    expect(info?.severity).toBe("info");
    expect(unknown?.severity).toBe("unknown");
  });

  it("rejects out-of-mirror URIs", () => {
    const result = normalizePublishDiagnostics(
      "file:///elsewhere/x.gd",
      [
        {
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
          message: "x",
        },
      ],
      context("x.gd"),
    );
    expect(result).toBeNull();
  });

  it("truncates excessive diagnostics explicitly", () => {
    const many = Array.from(
      { length: GODOT_LIMITS.lspMaxDiagnosticsPerDocument + 5 },
      (_, index) => ({
        range: { start: { line: index, character: 0 }, end: { line: index, character: 1 } },
        message: `issue ${index}`,
      }),
    );
    const result = normalizePublishDiagnostics(uri("a.gd"), many, context("a.gd"));
    expect(result?.diagnostics).toHaveLength(GODOT_LIMITS.lspMaxDiagnosticsPerDocument);
    expect(result?.truncated).toBe(true);
  });

  it("sanitizes control characters and mirror paths in messages", () => {
    const result = normalizePublishDiagnostics(
      uri("a.gd"),
      [
        {
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
          message: `bad \u001b[31m${MIRROR}/secret.gd`,
        },
      ],
      context("a.gd"),
    );
    const message = result?.diagnostics[0]?.message ?? "";
    expect(message).not.toContain("\u001b");
    expect(message).not.toContain(MIRROR);
  });

  it("updates replace prior state: later publishes normalize independently", () => {
    const first = normalizePublishDiagnostics(
      uri("a.gd"),
      [
        {
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
          message: "old",
        },
      ],
      context("a.gd"),
    );
    const second = normalizePublishDiagnostics(
      uri("a.gd"),
      [
        {
          range: { start: { line: 1, character: 0 }, end: { line: 1, character: 1 } },
          message: "new",
        },
      ],
      context("a.gd"),
    );
    expect(first?.diagnostics[0]?.message).toBe("old");
    expect(second?.diagnostics[0]?.message).toBe("new");
    expect(second?.diagnostics[0]?.line).toBe(2);
  });

  it("handles escaped/Unicode paths", () => {
    const result = normalizePublishDiagnostics(
      pathToFileUri(`${MIRROR}/src/плаyer/my file.gd`),
      [
        {
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
          message: "x",
        },
      ],
      context("src/плаyer/my file.gd"),
    );
    expect(result?.path).toBe("src/плаyer/my file.gd");
  });
});

describe("normalizeHover", () => {
  it("normalizes plaintext and markup content with 1-based ranges", () => {
    const result = normalizeHover(
      uri("src/player/player.gd"),
      {
        contents: { kind: "markdown", value: "# Player\n\nMoves the body." },
        range: { start: { line: 31, character: 13 }, end: { line: 31, character: 14 } },
      },
      context("src/player/player.gd"),
    );
    expect(result?.path).toBe("src/player/player.gd");
    expect(result?.range?.start).toEqual({ line: 32, column: 14 });
    expect(result?.contents[0]).toEqual({ kind: "markdown", text: "# Player\n\nMoves the body." });
  });

  it("supports null hover and array contents", () => {
    const nullResult = normalizeHover(uri("a.gd"), null, context("a.gd"));
    expect(nullResult).toBeNull();
    const arrayResult = normalizeHover(uri("a.gd"), { contents: ["one", "two"] }, context("a.gd"));
    expect(arrayResult?.contents.map((entry) => entry.text)).toEqual(["one", "two"]);
  });

  it("bounds hover content bytes", () => {
    const huge = "x".repeat(GODOT_LIMITS.lspMaxHoverBytes + 1024);
    const result = normalizeHover(uri("a.gd"), { contents: huge }, context("a.gd"));
    const total =
      result?.contents.reduce((sum, entry) => sum + Buffer.byteLength(entry.text, "utf8"), 0) ?? 0;
    expect(total).toBeLessThanOrEqual(GODOT_LIMITS.lspMaxHoverBytes);
  });

  it("treats embedded markup as data and omits mirror paths", () => {
    const result = normalizeHover(
      uri("a.gd"),
      { contents: `<script>alert(1)</script> ${MIRROR}/x` },
      context("a.gd"),
    );
    expect(result?.contents[0]?.text).toContain("<script>");
    expect(JSON.stringify(result)).not.toContain(MIRROR);
  });
});

describe("normalizeCompletion", () => {
  it("normalizes lists and arrays with bounds, dropping edits and commands", () => {
    const list = {
      isIncomplete: false,
      items: [
        {
          label: "move_and_slide",
          kind: 2,
          detail: "() -> bool",
          documentation: { kind: "markdown", value: "Moves the body." },
          insertText: "move_and_slide()",
          additionalTextEdits: [{ range: {}, newText: "EVIL" }],
          command: { title: "run", command: "evil" },
        },
        { label: 42 },
        { label: "second" },
      ],
    };
    const result = normalizeCompletion(
      uri("src/player/player.gd"),
      list,
      context("src/player/player.gd"),
    );
    expect(result.items).toHaveLength(2);
    expect(result.items[0]).toEqual({
      label: "move_and_slide",
      kind: "2",
      detail: "() -> bool",
      documentation: "Moves the body.",
      insertText: "move_and_slide()",
    });
    expect(JSON.stringify(result)).not.toContain("EVIL");
    expect(JSON.stringify(result)).not.toContain("evil");
  });

  it("bounds item counts", () => {
    const many = Array.from({ length: GODOT_LIMITS.lspMaxCompletionItems + 10 }, (_, index) => ({
      label: `item${index}`,
    }));
    const result = normalizeCompletion(uri("a.gd"), many, context("a.gd"));
    expect(result.items).toHaveLength(GODOT_LIMITS.lspMaxCompletionItems);
    expect(result.truncated).toBe(true);
  });

  it("never exposes server-internal absolute paths", () => {
    const result = normalizeCompletion(
      uri("a.gd"),
      [{ label: "x", detail: `${MIRROR}/internal`, documentation: `${MIRROR}/docs` }],
      context("a.gd"),
    );
    expect(JSON.stringify(result)).not.toContain(MIRROR);
  });
});

describe("normalizeDefinition", () => {
  it("maps single locations, arrays, and location links to workspace-relative paths", () => {
    const single = normalizeDefinition(
      uri("a.gd"),
      {
        uri: uri("src/player/player.gd"),
        range: { start: { line: 9, character: 0 }, end: { line: 9, character: 4 } },
      },
      context("a.gd"),
    );
    expect(single.locations[0]).toEqual({
      path: "src/player/player.gd",
      range: { start: { line: 10, column: 1 }, end: { line: 10, column: 5 } },
      external: false,
    });
    const array = normalizeDefinition(
      uri("a.gd"),
      [
        {
          uri: uri("src/ui/menu.gd"),
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
        },
      ],
      context("a.gd"),
    );
    expect(array.locations[0]?.path).toBe("src/ui/menu.gd");
    const link = normalizeDefinition(
      uri("a.gd"),
      [
        {
          targetUri: uri("src/player/player.gd"),
          targetRange: { start: { line: 1, character: 2 }, end: { line: 1, character: 3 } },
        },
      ],
      context("a.gd"),
    );
    expect(link.locations[0]?.path).toBe("src/player/player.gd");
    expect(link.locations[0]?.range.start).toEqual({ line: 2, column: 3 });
  });

  it("represents out-of-project locations conservatively without absolute paths", () => {
    const result = normalizeDefinition(
      uri("a.gd"),
      [
        {
          uri: "file:///engine/internal/class.gd",
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
        },
      ],
      context("a.gd"),
    );
    expect(result.locations[0]?.external).toBe(true);
    expect(JSON.stringify(result)).not.toContain("/engine/internal");
    expect(result.locations[0]?.path).toBe("class.gd");
  });

  it("bounds result counts", () => {
    const many = Array.from(
      { length: GODOT_LIMITS.lspMaxDefinitionLocations + 10 },
      (_, index) => ({
        uri: uri(`f${index}.gd`),
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
      }),
    );
    const result = normalizeDefinition(uri("a.gd"), many, context("a.gd"));
    expect(result.locations).toHaveLength(GODOT_LIMITS.lspMaxDefinitionLocations);
    expect(result.truncated).toBe(true);
  });

  it("never returns mirror absolute paths", () => {
    const result = normalizeDefinition(
      uri("a.gd"),
      [
        {
          uri: uri("b.gd"),
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
        },
      ],
      context("a.gd"),
    );
    expect(JSON.stringify(result)).not.toContain(MIRROR);
  });
});
