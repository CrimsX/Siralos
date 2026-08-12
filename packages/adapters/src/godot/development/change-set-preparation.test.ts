import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { applyReplacements, prepareChangeSet, hashText } from "./change-set-preparation.js";
import {
  createTempWorkspace,
  SYMLINKS_SUPPORTED,
  createSymlink,
  writeFixtureFiles,
  type TempWorkspace,
} from "../../tools/workspace/workspace-fixtures.js";

const PLAYER_BEFORE = "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tpass\n";

function sha256Of(text: string): string {
  return createHash("sha256").update(text).digest("hex");
}

describe("applyReplacements", () => {
  it("replaces all non-overlapping occurrences in order", () => {
    expect(applyReplacements("a b a b", [{ oldText: "a", newText: "x" }])).toEqual({
      ok: true,
      text: "x b x b",
    });
  });

  it("applies multiple replacements sequentially", () => {
    expect(
      applyReplacements("a b", [
        { oldText: "a", newText: "c" },
        { oldText: "c b", newText: "d" },
      ]),
    ).toEqual({ ok: true, text: "d" });
  });

  it("rejects a replacement whose oldText does not occur", () => {
    const result = applyReplacements("hello", [{ oldText: "missing", newText: "x" }]);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.message).toContain("does not occur");
    }
  });
});

describe("prepareChangeSet", () => {
  let workspace: TempWorkspace;
  beforeEach(async () => {
    workspace = await createTempWorkspace();
  });
  afterEach(async () => {
    await workspace.cleanup();
  });

  const deps = () => ({ workspaceRoot: workspace.root });

  it("prepares a one-file edit with exact replacements and a complete diff", async () => {
    await writeFixtureFiles(workspace.root, {
      "src/player/player.gd": PLAYER_BEFORE,
    });
    const result = await prepareChangeSet(
      {
        changes: [
          {
            operation: "edit",
            path: "src/player/player.gd",
            expectedSha256: sha256Of(PLAYER_BEFORE),
            replacements: [{ oldText: "pass", newText: "move_and_slide()" }],
          },
        ],
      },
      deps(),
    );
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    expect(result.files).toHaveLength(1);
    const file = result.files[0]!;
    expect(file.operation).toBe("update");
    expect(file.beforeSha256).toBe(sha256Of(PLAYER_BEFORE));
    expect(file.afterSha256).toBe(sha256Of(PLAYER_BEFORE.replace("pass", "move_and_slide()")));
    expect(file.content).toContain("move_and_slide()");
    expect(file.unifiedDiff).toContain("-\tpass");
    expect(file.unifiedDiff).toContain("+\tmove_and_slide()");
    expect(result.preview.files[0]?.addedLines).toBeGreaterThan(0);
    expect(result.preview.truncated).toBe(false);
    expect(result.digest).toMatch(/^[0-9a-f]{64}$/);
  });

  it("prepares create + edit + delete together", async () => {
    await writeFixtureFiles(workspace.root, {
      "src/player/player.gd": PLAYER_BEFORE,
      "src/player/old.gd": "extends Node\n",
    });
    const result = await prepareChangeSet(
      {
        changes: [
          { operation: "create", path: "src/player/health.gd", content: "extends Node\n" },
          {
            operation: "edit",
            path: "src/player/player.gd",
            expectedSha256: sha256Of(PLAYER_BEFORE),
            replacements: [{ oldText: "pass", newText: "move_and_slide()" }],
          },
          {
            operation: "delete",
            path: "src/player/old.gd",
            expectedSha256: sha256Of("extends Node\n"),
          },
        ],
      },
      deps(),
    );
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    expect(result.files.map((file) => file.operation)).toEqual(["create", "update", "delete"]);
    expect(result.files[0]?.beforeSha256).toBeNull();
    expect(result.files[2]?.afterSha256).toBeNull();
  });

  it("conflicts when an edit's expected hash is stale", async () => {
    await writeFixtureFiles(workspace.root, { "a.gd": "one\n" });
    const result = await prepareChangeSet(
      {
        changes: [
          {
            operation: "edit",
            path: "a.gd",
            expectedSha256: "f".repeat(64),
            replacements: [{ oldText: "one", newText: "two" }],
          },
        ],
      },
      deps(),
    );
    expect(result.status).toBe("conflict");
    if (result.status === "conflict") {
      expect(result.message).toContain("changed since");
    }
  });

  it("conflicts when the create target already exists", async () => {
    await writeFixtureFiles(workspace.root, { "a.gd": "one\n" });
    const result = await prepareChangeSet(
      { changes: [{ operation: "create", path: "a.gd", content: "two\n" }] },
      deps(),
    );
    expect(result.status).toBe("conflict");
  });

  it("rejects a delete with a stale expected hash", async () => {
    await writeFixtureFiles(workspace.root, { "a.gd": "one\n" });
    const result = await prepareChangeSet(
      {
        changes: [{ operation: "delete", path: "a.gd", expectedSha256: "f".repeat(64) }],
      },
      deps(),
    );
    expect(result.status).toBe("conflict");
  });

  it("rejects protected paths for the whole set", async () => {
    await writeFixtureFiles(workspace.root, { ".env": "SECRET=1\n", "ok.gd": "one\n" });
    const result = await prepareChangeSet(
      {
        changes: [
          {
            operation: "edit",
            path: ".env",
            expectedSha256: sha256Of("SECRET=1\n"),
            replacements: [{ oldText: "1", newText: "2" }],
          },
        ],
      },
      deps(),
    );
    expect(result.status).toBe("invalid_input");
    if (result.status === "invalid_input") {
      expect(result.message).toContain("protected");
    }
  });

  it("rejects protected behavioral configuration for the whole set", async () => {
    await writeFixtureFiles(workspace.root, {
      "AGENTS.md": "Root guidance.\n",
      "src/player/player.gd": "extends Node\n",
    });
    for (const path of ["AGENTS.md", "src/AGENTS.md", ".siralos/config.json"]) {
      const result = await prepareChangeSet(
        {
          changes: [
            { operation: "create", path, content: "new content" },
            {
              operation: "edit",
              path: "src/player/player.gd",
              expectedSha256: sha256Of("extends Node\n"),
              replacements: [{ oldText: "Node", newText: "Node2D" }],
            },
          ],
        },
        deps(),
      );
      expect(result.status).toBe("invalid_input");
      if (result.status === "invalid_input") {
        expect(result.message).toContain("protected behavioral configuration");
        expect(result.message).toContain("before any write, approval, or checkpoint");
      }
      // The file was never touched: preparation is read-only and rejected.
      expect(await readFile(join(workspace.root, "src/player/player.gd"), "utf8")).toBe(
        "extends Node\n",
      );
    }
  });

  it("rejects paths escaping the workspace", async () => {
    const result = await prepareChangeSet(
      { changes: [{ operation: "create", path: "../escape.gd", content: "x" }] },
      deps(),
    );
    expect(result.status).toBe("invalid_input");
  });

  it("rejects a symlink target", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    await writeFixtureFiles(workspace.root, { "real.gd": "one\n" });
    await createSymlink("real.gd", join(workspace.root, "link.gd"));
    const result = await prepareChangeSet(
      {
        changes: [
          {
            operation: "edit",
            path: "link.gd",
            expectedSha256: sha256Of("one\n"),
            replacements: [{ oldText: "one", newText: "two" }],
          },
        ],
      },
      deps(),
    );
    expect(result.status).toBe("invalid_input");
  });

  it("rejects non-text and oversized files", async () => {
    await writeFixtureFiles(workspace.root, { "bin.gd": Buffer.from([0, 1, 2, 3]) });
    const binaryResult = await prepareChangeSet(
      {
        changes: [
          {
            operation: "edit",
            path: "bin.gd",
            expectedSha256: sha256Of("\u0000\u0001\u0002\u0003"),
            replacements: [{ oldText: "\u0000", newText: "x" }],
          },
        ],
      },
      deps(),
    );
    expect(binaryResult.status).toBe("invalid_input");
    await writeFixtureFiles(workspace.root, { "big.gd": "x".repeat(1024 * 1024 + 1) });
    const oversizedResult = await prepareChangeSet(
      {
        changes: [
          {
            operation: "edit",
            path: "big.gd",
            expectedSha256: sha256Of("x".repeat(1024 * 1024 + 1)),
            replacements: [{ oldText: "xxx", newText: "y" }],
          },
        ],
      },
      deps(),
    );
    expect(oversizedResult.status).toBe("invalid_input");
  });

  it("rejects a change set whose complete diff exceeds the limit", async () => {
    // Three files whose full diffs individually fit the per-file bound but
    // whose sum exceeds the 512 KiB complete-diff bound.
    const line = "y".repeat(500);
    const longLine = `${line}\n`;
    const fileNames = ["f-0.gd", "f-1.gd", "f-2.gd"];
    const files: Record<string, string> = {};
    const changes = fileNames.map((name) => {
      files[name] = longLine.repeat(200);
      return {
        operation: "edit" as const,
        path: name,
        expectedSha256: sha256Of(longLine.repeat(200)),
        replacements: [{ oldText: line, newText: `${line}z` }],
      };
    });
    await writeFixtureFiles(workspace.root, files);
    const result = await prepareChangeSet({ changes }, deps());
    expect(result.status).toBe("changeset_too_large");
  });

  it("is deterministic: equal requests produce equal digests and previews", async () => {
    await writeFixtureFiles(workspace.root, { "a.gd": "one\n" });
    const input = {
      changes: [
        {
          operation: "edit" as const,
          path: "a.gd",
          expectedSha256: sha256Of("one\n"),
          replacements: [{ oldText: "one", newText: "two" }],
        },
      ],
    };
    const first = await prepareChangeSet(input, deps());
    const second = await prepareChangeSet(input, deps());
    expect(first.status).toBe("ready");
    expect(second.status).toBe("ready");
    if (first.status === "ready" && second.status === "ready") {
      expect(first.digest).toBe(second.digest);
      expect(first.preview).toEqual(second.preview);
      expect(first.files).toEqual(second.files);
    }
  });

  it("computes hashText over UTF-8 bytes", () => {
    expect(hashText("\u00e9")).toBe(sha256Of("\u00e9"));
  });
});
