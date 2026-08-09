import { describe, expect, it } from "vitest";
import {
  computeChangeSetDigest,
  countChangeSetResultBytes,
  validateChangeSetRequest,
  type PreparedChangeSetDigestParts,
  type PreparedChangeSetFile,
} from "./development-change-set.js";

const GOOD_EDIT = {
  operation: "edit",
  path: "src/player/player.gd",
  expectedSha256: "a".repeat(64),
  replacements: [{ oldText: "old", newText: "new" }],
};

const GOOD_CREATE = {
  operation: "create",
  path: "src/player/health.gd",
  content: "extends Node\n",
};

const GOOD_DELETE = {
  operation: "delete",
  path: "src/player/old.gd",
  expectedSha256: "b".repeat(64),
};

describe("validateChangeSetRequest", () => {
  it("accepts a one-file edit", () => {
    const result = validateChangeSetRequest({ changes: [GOOD_EDIT] });
    expect(result.ok).toBe(true);
  });

  it("accepts create + edit + delete mixtures", () => {
    const result = validateChangeSetRequest({
      changes: [GOOD_CREATE, GOOD_EDIT, GOOD_DELETE],
    });
    expect(result.ok).toBe(true);
  });

  it("rejects non-object input", () => {
    expect(validateChangeSetRequest(null).ok).toBe(false);
    expect(validateChangeSetRequest([]).ok).toBe(false);
    expect(validateChangeSetRequest("nope").ok).toBe(false);
  });

  it("rejects an empty change list", () => {
    const result = validateChangeSetRequest({ changes: [] });
    expect(result.ok).toBe(false);
  });

  it("enforces the file-count limit", () => {
    const changes = Array.from({ length: 17 }, (_, index) => ({
      operation: "create",
      path: `file-${index}.gd`,
      content: "extends Node\n",
    }));
    const result = validateChangeSetRequest({ changes });
    expect(result.ok).toBe(false);
    expect(result.ok || result.message).toContain("16");
  });

  it("rejects duplicate paths within one change set", () => {
    const result = validateChangeSetRequest({ changes: [GOOD_EDIT, GOOD_EDIT] });
    expect(result.ok).toBe(false);
    expect(result.ok || result.message).toContain("more than once");
  });

  it("rejects edits without the exact 64-hex-digit expected hash", () => {
    for (const expectedSha256 of ["", "short", "g".repeat(64), null, 42]) {
      const result = validateChangeSetRequest({
        changes: [{ ...GOOD_EDIT, expectedSha256 }],
      });
      expect(result.ok).toBe(false);
    }
  });

  it("rejects creates without string content", () => {
    const result = validateChangeSetRequest({
      changes: [{ ...GOOD_CREATE, content: 42 }],
    });
    expect(result.ok).toBe(false);
  });

  it("rejects empty replacement oldText", () => {
    const result = validateChangeSetRequest({
      changes: [
        { ...GOOD_EDIT, replacements: [{ oldText: "", newText: "x" }] },
      ],
    });
    expect(result.ok).toBe(false);
  });

  it("enforces the replacements-per-file limit", () => {
    const replacements = Array.from({ length: 33 }, () => ({ oldText: "a", newText: "b" }));
    const result = validateChangeSetRequest({
      changes: [{ ...GOOD_EDIT, replacements }],
    });
    expect(result.ok).toBe(false);
  });

  it("rejects unknown operations", () => {
    const result = validateChangeSetRequest({
      changes: [{ operation: "replace", path: "a.gd" }],
    });
    expect(result.ok).toBe(false);
  });

  it("normalizes trimmed paths", () => {
    const result = validateChangeSetRequest({
      changes: [{ ...GOOD_EDIT, path: "  src/player/player.gd  " }],
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.request.changes[0]?.path).toBe("src/player/player.gd");
    }
  });
});

describe("computeChangeSetDigest", () => {
  const parts: PreparedChangeSetDigestParts = {
    changes: [
      { operation: "update", path: "a.gd", beforeSha256: "a".repeat(64), afterSha256: "b".repeat(64) },
      { operation: "create", path: "b.gd", beforeSha256: null, afterSha256: "c".repeat(64) },
    ],
  };

  it("is deterministic for equal structures regardless of key order", () => {
    expect(computeChangeSetDigest(parts)).toBe(
      computeChangeSetDigest({
        changes: [
          {
            afterSha256: "b".repeat(64),
            beforeSha256: "a".repeat(64),
            operation: "update",
            path: "a.gd",
          },
          {
            afterSha256: "c".repeat(64),
            beforeSha256: null,
            operation: "create",
            path: "b.gd",
          },
        ],
      }),
    );
  });

  it("binds before and after hashes", () => {
    expect(
      computeChangeSetDigest({
        changes: [
          {
            operation: "update",
            path: "a.gd",
            beforeSha256: "x".repeat(64),
            afterSha256: "b".repeat(64),
          },
        ],
      }),
    ).not.toBe(
      computeChangeSetDigest({
        changes: [
          {
            operation: "update",
            path: "a.gd",
            beforeSha256: "a".repeat(64),
            afterSha256: "b".repeat(64),
          },
        ],
      }),
    );
  });
});

describe("countChangeSetResultBytes", () => {
  const file = (
    operation: "create" | "update" | "delete",
    content: string | null,
  ): PreparedChangeSetFile => ({
    path: "x.gd",
    operation,
    expectedSha256: operation === "create" ? null : "a".repeat(64),
    content,
    beforeSha256: operation === "create" ? null : "a".repeat(64),
    afterSha256: operation === "delete" ? null : "b".repeat(64),
    unifiedDiff: "",
    addedLines: 0,
    removedLines: 0,
  });

  it("counts only resulting file bytes", () => {
    const files = [file("update", "hello"), file("create", "world"), file("delete", null)];
    expect(countChangeSetResultBytes(files)).toBe(10);
  });

  it("counts UTF-8 bytes, not characters", () => {
    expect(countChangeSetResultBytes([file("create", "\u00e9")])).toBe(2);
  });
});
