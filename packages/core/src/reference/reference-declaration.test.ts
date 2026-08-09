import { describe, expect, it } from "vitest";
import {
  normalizeRepositoryOrigin,
  parseReferenceDeclaration,
  parseReferenceDeclarationsSection,
} from "./reference-declaration.js";
import { REFERENCE_LIMITS, validateReferenceAlias } from "./reference-model.js";

describe("reference alias validation", () => {
  it("accepts valid lowercase aliases", () => {
    expect(validateReferenceAlias("docs")).toEqual("docs");
    expect(validateReferenceAlias("godot-docs")).toEqual("godot-docs");
    expect(validateReferenceAlias("a1.b_c-d")).toEqual("a1.b_c-d");
    expect(validateReferenceAlias("x".repeat(64))).toEqual("x".repeat(64));
  });

  it("rejects malformed aliases", () => {
    expect(validateReferenceAlias("")).toBeNull();
    expect(validateReferenceAlias("A")).toBeNull();
    expect(validateReferenceAlias("1docs")).toBeNull();
    expect(validateReferenceAlias("docs!")).toBeNull();
    expect(validateReferenceAlias("d")).toBeNull(); // min length 2
    expect(validateReferenceAlias("x".repeat(65))).toBeNull(); // max length 64
    expect(validateReferenceAlias(42)).toBeNull();
    expect(validateReferenceAlias(null)).toBeNull();
    expect(validateReferenceAlias("with space")).toBeNull();
  });
});

describe("parseReferenceDeclaration", () => {
  it("parses a local-directory declaration", () => {
    const result = parseReferenceDeclaration({
      alias: "assets",
      kind: "local-directory",
      source: { kind: "local-directory", path: "/srv/shared-assets" },
      description: "Shared art assets",
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value.alias).toBe("assets");
      expect(result.value.kind).toBe("local-directory");
      expect(result.value.source).toEqual({
        kind: "local-directory",
        path: "/srv/shared-assets",
      });
      expect(result.value.description).toBe("Shared art assets");
    }
  });

  it("parses a repository declaration with a pinned commit", () => {
    const result = parseReferenceDeclaration({
      alias: "gdscript-docs",
      kind: "repository",
      source: {
        kind: "repository",
        repository: "godotengine/godot-docs",
        ref: { kind: "commit", commit: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0" },
      },
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value.source).toEqual({
        kind: "repository",
        repository: "https://github.com/godotengine/godot-docs",
        ref: { kind: "commit", commit: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0" },
      });
    }
  });

  it("defaults an absent ref to the mutable branch main", () => {
    const result = parseReferenceDeclaration({
      alias: "docs",
      kind: "repository",
      source: { kind: "repository", repository: "godotengine/godot" },
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value.source).toEqual({
        kind: "repository",
        repository: "https://github.com/godotengine/godot",
        ref: { kind: "branch", branch: "main" },
      });
    }
  });

  it("rejects unknown keys naming the offending key (secrets cannot hide)", () => {
    const result = parseReferenceDeclaration({
      alias: "docs",
      kind: "repository",
      source: {
        kind: "repository",
        repository: "godotengine/godot",
        ref: { kind: "branch", branch: "main" },
      },
      description: null,
      apiKey: "sk-secret-value",
    });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.reason).toContain("apiKey");
    }
  });

  it("rejects secret-shaped extra keys inside the source and ref", () => {
    const sourceSecret = parseReferenceDeclaration({
      alias: "docs",
      kind: "repository",
      source: { kind: "repository", repository: "godotengine/godot", token: "ghp_12345" },
    });
    expect(sourceSecret.ok).toBe(false);
    if (!sourceSecret.ok) {
      expect(sourceSecret.reason).toContain("token");
    }
    const refSecret = parseReferenceDeclaration({
      alias: "docs",
      kind: "repository",
      source: {
        kind: "repository",
        repository: "godotengine/godot",
        ref: { kind: "branch", branch: "main", password: "hunter2" },
      },
    });
    expect(refSecret.ok).toBe(false);
    if (!refSecret.ok) {
      expect(refSecret.reason).toContain("password");
    }
  });

  it("rejects non-object values and arrays", () => {
    expect(parseReferenceDeclaration("docs").ok).toBe(false);
    expect(parseReferenceDeclaration(null).ok).toBe(false);
    expect(parseReferenceDeclaration(["docs"]).ok).toBe(false);
  });

  it("rejects malformed aliases and kinds", () => {
    const badAlias = parseReferenceDeclaration({
      alias: "Docs!",
      kind: "local-directory",
      source: { kind: "local-directory", path: "/x" },
    });
    expect(badAlias.ok).toBe(false);
    const badKind = parseReferenceDeclaration({
      alias: "docs",
      kind: "symlink",
      source: { kind: "local-directory", path: "/x" },
    });
    expect(badKind.ok).toBe(false);
  });

  it("rejects non-absolute local-directory paths without resolving them", () => {
    const relative = parseReferenceDeclaration({
      alias: "docs",
      kind: "local-directory",
      source: { kind: "local-directory", path: "relative/path" },
    });
    expect(relative.ok).toBe(false);
    if (!relative.ok) {
      expect(relative.reason).toContain("not absolute");
    }
    const tilde = parseReferenceDeclaration({
      alias: "docs",
      kind: "local-directory",
      source: { kind: "local-directory", path: "~/docs" },
    });
    expect(tilde.ok).toBe(false);
  });

  it("accepts POSIX, Windows drive, and UNC absolute paths", () => {
    const posix = parseReferenceDeclaration({
      alias: "docs",
      kind: "local-directory",
      source: { kind: "local-directory", path: "/home/user/docs" },
    });
    expect(posix.ok).toBe(true);
    const windows = parseReferenceDeclaration({
      alias: "docs",
      kind: "local-directory",
      source: { kind: "local-directory", path: "C:\\Users\\user\\docs" },
    });
    expect(windows.ok).toBe(true);
    const unc = parseReferenceDeclaration({
      alias: "docs",
      kind: "local-directory",
      source: { kind: "local-directory", path: "\\\\server\\share\\docs" },
    });
    expect(unc.ok).toBe(true);
  });

  it("rejects null bytes in paths", () => {
    const result = parseReferenceDeclaration({
      alias: "docs",
      kind: "local-directory",
      source: { kind: "local-directory", path: "/docs\0hidden" },
    });
    expect(result.ok).toBe(false);
  });

  it("bounds the description", () => {
    const overlong = parseReferenceDeclaration({
      alias: "docs",
      kind: "local-directory",
      source: { kind: "local-directory", path: "/x" },
      description: "x".repeat(REFERENCE_LIMITS.maxDescriptionBytes + 1),
    });
    expect(overlong.ok).toBe(false);
  });

  it("rejects malformed commit pins and overlong branches/tags", () => {
    const shortCommit = parseReferenceDeclaration({
      alias: "docs",
      kind: "repository",
      source: {
        kind: "repository",
        repository: "godotengine/godot",
        ref: { kind: "commit", commit: "abc" },
      },
    });
    expect(shortCommit.ok).toBe(false);
    const longCommit = parseReferenceDeclaration({
      alias: "docs",
      kind: "repository",
      source: {
        kind: "repository",
        repository: "godotengine/godot",
        ref: { kind: "commit", commit: "a".repeat(65) },
      },
    });
    expect(longCommit.ok).toBe(false);
    const badBranch = parseReferenceDeclaration({
      alias: "docs",
      kind: "repository",
      source: {
        kind: "repository",
        repository: "godotengine/godot",
        ref: { kind: "branch", branch: "main branch" },
      },
    });
    expect(badBranch.ok).toBe(false);
    const longTag = parseReferenceDeclaration({
      alias: "docs",
      kind: "repository",
      source: {
        kind: "repository",
        repository: "godotengine/godot",
        ref: { kind: "tag", tag: "t".repeat(REFERENCE_LIMITS.maxTagLength + 1) },
      },
    });
    expect(longTag.ok).toBe(false);
  });
});

describe("normalizeRepositoryOrigin", () => {
  it("normalizes owner/repo shorthand to the canonical https form", () => {
    expect(normalizeRepositoryOrigin("godotengine/godot")).toEqual({
      ok: true,
      origin: "https://github.com/godotengine/godot",
    });
  });

  it("normalizes https forms and strips .git and trailing slashes", () => {
    expect(normalizeRepositoryOrigin("https://github.com/godotengine/godot")).toEqual({
      ok: true,
      origin: "https://github.com/godotengine/godot",
    });
    expect(normalizeRepositoryOrigin("https://github.com/godotengine/godot.git")).toEqual({
      ok: true,
      origin: "https://github.com/godotengine/godot",
    });
    expect(normalizeRepositoryOrigin("https://github.com/godotengine/godot/")).toEqual({
      ok: true,
      origin: "https://github.com/godotengine/godot",
    });
    expect(normalizeRepositoryOrigin("godotengine/godot.git")).toEqual({
      ok: true,
      origin: "https://github.com/godotengine/godot",
    });
  });

  it("rejects other hosts and http", () => {
    expect(normalizeRepositoryOrigin("https://gitlab.com/group/repo").ok).toBe(false);
    expect(normalizeRepositoryOrigin("https://github.example.com/owner/repo").ok).toBe(false);
    expect(normalizeRepositoryOrigin("http://github.com/owner/repo").ok).toBe(false);
  });

  it("rejects credentials (userinfo)", () => {
    expect(normalizeRepositoryOrigin("user@github.com/owner/repo").ok).toBe(false);
    expect(normalizeRepositoryOrigin("https://user:pass@github.com/owner/repo").ok).toBe(false);
    expect(normalizeRepositoryOrigin("https://user@github.com/owner/repo").ok).toBe(false);
  });

  it("rejects query strings, fragments, and extra path segments", () => {
    expect(normalizeRepositoryOrigin("https://github.com/owner/repo?tab=readme").ok).toBe(false);
    expect(normalizeRepositoryOrigin("https://github.com/owner/repo#readme").ok).toBe(false);
    expect(normalizeRepositoryOrigin("https://github.com/owner/repo/tree/main").ok).toBe(false);
    expect(normalizeRepositoryOrigin("github.com/owner/repo").ok).toBe(false);
  });

  it("rejects empty and malformed owner/repo", () => {
    expect(normalizeRepositoryOrigin("").ok).toBe(false);
    expect(normalizeRepositoryOrigin("/repo").ok).toBe(false);
    expect(normalizeRepositoryOrigin("owner/").ok).toBe(false);
    expect(normalizeRepositoryOrigin("owner").ok).toBe(false);
    expect(normalizeRepositoryOrigin("own_er/repo").ok).toBe(false); // underscores not allowed in owner
    expect(normalizeRepositoryOrigin("owner/repo-name.with.dots").ok).toBe(true);
    expect(normalizeRepositoryOrigin("https://github.com/owner/repo/").ok).toBe(true);
  });

  it("rejects null bytes", () => {
    expect(normalizeRepositoryOrigin("owner/repo\0x").ok).toBe(false);
  });
});

describe("parseReferenceDeclarationsSection", () => {
  it("parses a section mapping alias to declaration", () => {
    const result = parseReferenceDeclarationsSection({
      docs: {
        alias: "docs",
        kind: "repository",
        source: {
          kind: "repository",
          repository: "godotengine/godot",
          ref: { kind: "branch", branch: "main" },
        },
      },
      assets: {
        alias: "assets",
        kind: "local-directory",
        source: { kind: "local-directory", path: "/srv/assets" },
      },
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.declarations.map((declaration) => declaration.alias)).toEqual([
        "docs",
        "assets",
      ]);
    }
  });

  it("rejects an alias that does not match its key", () => {
    const result = parseReferenceDeclarationsSection({
      docs: {
        alias: "other",
        kind: "local-directory",
        source: { kind: "local-directory", path: "/x" },
      },
    });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.reason).toContain("does not match its key");
    }
  });

  it("rejects invalid keys and bounds the section size", () => {
    const badKey = parseReferenceDeclarationsSection({
      "Not-Aliased!": {
        alias: "Not-Aliased!",
        kind: "local-directory",
        source: { kind: "local-directory", path: "/x" },
      },
    });
    expect(badKey.ok).toBe(false);
    const many: Record<string, unknown> = {};
    for (let index = 0; index < REFERENCE_LIMITS.maxReferences + 1; index += 1) {
      many[`ref${index}`] = {
        alias: `ref${index}`,
        kind: "local-directory",
        source: { kind: "local-directory", path: "/x" },
      };
    }
    const over = parseReferenceDeclarationsSection(many);
    expect(over.ok).toBe(false);
    if (!over.ok) {
      expect(over.reason).toContain("limit");
    }
  });
});
