import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { DEFAULT_USER_CONFIG, loadUserConfig, parseUserConfig } from "./user-config.js";

const tempDirectories: string[] = [];
const absoluteGodotPath = join(tmpdir(), "godot.exe");
const absoluteVersionedGodotPath = join(tmpdir(), "Godot_v4.7.1-stable.exe");

async function withConfigFile(content: string): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "siralos-config-"));
  tempDirectories.push(directory);
  const path = join(directory, "config.json");
  await writeFile(path, content);
  return path;
}

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

describe("parseUserConfig", () => {
  it("defaults to the inspect profile when the sandbox section is missing", () => {
    expect(parseUserConfig({})).toEqual(DEFAULT_USER_CONFIG);
  });

  it("loads a valid develop-offline configuration", () => {
    expect(
      parseUserConfig({ sandbox: { profile: "develop-offline", backend: "anthropic-runtime" } }),
    ).toEqual({
      sandbox: { profile: "develop-offline", backend: "anthropic-runtime" },
      godot: DEFAULT_USER_CONFIG.godot,
      quality: DEFAULT_USER_CONFIG.quality,
      references: DEFAULT_USER_CONFIG.references,
    });
  });

  it("accepts partial sandbox sections with defaults", () => {
    expect(parseUserConfig({ sandbox: { profile: "develop-offline" } })).toEqual({
      sandbox: { profile: "develop-offline", backend: "auto" },
      godot: DEFAULT_USER_CONFIG.godot,
      quality: DEFAULT_USER_CONFIG.quality,
      references: DEFAULT_USER_CONFIG.references,
    });
    expect(parseUserConfig({ sandbox: {} })).toEqual(DEFAULT_USER_CONFIG);
  });

  it("rejects unknown profiles", () => {
    expect(() => parseUserConfig({ sandbox: { profile: "full-access" } })).toThrow(
      "Unknown sandbox profile",
    );
  });

  it("accepts a configured quality.reviewProvider profile reference", () => {
    expect(parseUserConfig({ quality: { reviewProvider: "reviewer" } })).toEqual({
      sandbox: DEFAULT_USER_CONFIG.sandbox,
      godot: DEFAULT_USER_CONFIG.godot,
      quality: { reviewProvider: "reviewer" },
      references: DEFAULT_USER_CONFIG.references,
    });
  });

  it("defaults quality.reviewProvider to null", () => {
    expect(parseUserConfig({}).quality.reviewProvider).toBeNull();
    expect(parseUserConfig({ quality: {} }).quality.reviewProvider).toBeNull();
  });

  it("rejects malformed quality.reviewProvider values", () => {
    for (const value of ["", "bad provider", "spaces are bad", "a".repeat(200), 42, {}]) {
      expect(() => parseUserConfig({ quality: { reviewProvider: value } })).toThrow(
        "quality.reviewProvider",
      );
    }
  });

  it("rejects unknown quality keys", () => {
    expect(() => parseUserConfig({ quality: { reviewProviders: ["x"] } })).toThrow(
      "Unknown Siralos quality configuration key",
    );
    expect(() => parseUserConfig({ quality: "reviewer" })).toThrow(
      'Siralos configuration section "quality" must be a JSON object',
    );
  });

  it("rejects unknown backends", () => {
    expect(() => parseUserConfig({ sandbox: { backend: "docker" } })).toThrow(
      "Unknown sandbox backend",
    );
  });

  it("rejects unknown top-level sections", () => {
    expect(() => parseUserConfig({ permissions: {} })).toThrow(
      "Unknown Siralos configuration section",
    );
  });

  it("rejects unknown sandbox keys", () => {
    expect(() => parseUserConfig({ sandbox: { networkAllowlist: [] } })).toThrow(
      "Unknown Siralos sandbox configuration key",
    );
  });

  it("rejects non-object configuration", () => {
    expect(() => parseUserConfig("config")).toThrow();
    expect(() => parseUserConfig([1, 2])).toThrow();
  });

  it("cannot contain credentials", () => {
    expect(() => parseUserConfig({ sandbox: { apiKey: "secret" } })).toThrow(
      "Unknown Siralos sandbox configuration key",
    );
  });
});

describe("parseUserConfig references section", () => {
  it("defaults the references section to an empty map", () => {
    expect(parseUserConfig({}).references).toEqual({});
    expect(DEFAULT_USER_CONFIG.references).toEqual({});
  });

  it("loads a valid local-directory reference as raw unknown values", () => {
    const config = parseUserConfig({
      references: {
        "godot-src": {
          kind: "local-directory",
          path: "C:\\External\\godot",
          description: "The Godot engine source",
        },
      },
    });
    expect(config.references["godot-src"]).toEqual({
      kind: "local-directory",
      path: "C:\\External\\godot",
      description: "The Godot engine source",
    });
  });

  it("loads a valid repository reference with a commit ref", () => {
    const config = parseUserConfig({
      references: {
        "godot-engine": {
          kind: "repository",
          repository: "godotengine/godot",
          ref: { kind: "commit", commit: "0123456789abcdef0123456789abcdef01234567" },
        },
      },
    });
    expect(config.references["godot-engine"]).toEqual({
      kind: "repository",
      repository: "godotengine/godot",
      ref: { kind: "commit", commit: "0123456789abcdef0123456789abcdef01234567" },
    });
  });

  it("accepts a repository reference without a ref (defaults to main; core refuses it without a pin)", () => {
    const config = parseUserConfig({
      references: { "godot-engine": { kind: "repository", repository: "godotengine/godot" } },
    });
    expect(config.references["godot-engine"]).toEqual({
      kind: "repository",
      repository: "godotengine/godot",
    });
  });

  it("rejects unknown declaration keys (a credential field cannot hide)", () => {
    expect(() =>
      parseUserConfig({
        references: { "godot-src": { kind: "local-directory", path: "C:\\x", apiKey: "secret" } },
      }),
    ).toThrow('Unknown Siralos reference key: apiKey (reference "godot-src").');
  });

  it("rejects unknown ref keys", () => {
    expect(() =>
      parseUserConfig({
        references: {
          "godot-engine": {
            kind: "repository",
            repository: "godotengine/godot",
            ref: { kind: "commit", commit: "abc1234", token: "secret" },
          },
        },
      }),
    ).toThrow('Unknown Siralos reference ref key: token (reference "godot-engine").');
  });

  it("rejects a ref that pins more than one of commit/tag/branch", () => {
    expect(() =>
      parseUserConfig({
        references: {
          "godot-engine": {
            kind: "repository",
            repository: "godotengine/godot",
            ref: { kind: "commit", commit: "abc1234", tag: "4.3" },
          },
        },
      }),
    ).toThrow("a ref pins exactly one of commit/tag/branch");
  });

  it("rejects malformed alias keys", () => {
    for (const alias of ["", "Uppercase", "has space", "a".repeat(65), "9starts-with-digit"]) {
      expect(() =>
        parseUserConfig({ references: { [alias]: { kind: "local-directory", path: "C:\\x" } } }),
      ).toThrow("Reference alias");
    }
  });

  it("rejects more than 16 references", () => {
    const references: Record<string, unknown> = {};
    for (let index = 0; index < 17; index += 1) {
      references[`ref-${index}`] = { kind: "local-directory", path: "C:\\x" };
    }
    expect(() => parseUserConfig({ references })).toThrow("the limit is 16");
  });

  it("rejects unknown kinds and missing per-kind required fields", () => {
    expect(() => parseUserConfig({ references: { xx: { kind: "git" } } })).toThrow(
      '"kind" of "local-directory" or "repository"',
    );
    expect(() => parseUserConfig({ references: { xx: { kind: "local-directory" } } })).toThrow(
      'requires a non-empty "path"',
    );
    expect(() => parseUserConfig({ references: { xx: { kind: "repository" } } })).toThrow(
      'requires a non-empty "repository"',
    );
    expect(() =>
      parseUserConfig({
        references: { xx: { kind: "repository", repository: "a/b", ref: { kind: "tag" } } },
      }),
    ).toThrow("ref requires a non-empty tag string");
  });

  it("rejects a local-directory declaration that also carries repository fields", () => {
    expect(() =>
      parseUserConfig({
        references: {
          xx: { kind: "local-directory", path: "C:\\x", repository: "a/b" },
        },
      }),
    ).toThrow('must not declare "repository" or "ref"');
  });

  it("rejects a non-object references section", () => {
    expect(() => parseUserConfig({ references: "none" })).toThrow(
      'Siralos configuration section "references" must be a JSON object',
    );
    expect(() => parseUserConfig({ references: [] })).toThrow(
      'Siralos configuration section "references" must be a JSON object',
    );
  });

  it("rejects a non-string description", () => {
    expect(() =>
      parseUserConfig({
        references: { xx: { kind: "local-directory", path: "C:\\x", description: 5 } },
      }),
    ).toThrow("description must be a string");
  });
});

describe("loadUserConfig", () => {
  it("returns defaults when no config file exists", async () => {
    const directory = await mkdtemp(join(tmpdir(), "siralos-config-"));
    tempDirectories.push(directory);
    const config = await loadUserConfig(join(directory, "missing.json"));
    expect(config).toEqual(DEFAULT_USER_CONFIG);
  });

  it("loads a valid config file", async () => {
    const path = await withConfigFile(
      JSON.stringify({ sandbox: { profile: "develop-offline", backend: "anthropic-runtime" } }),
    );
    const config = await loadUserConfig(path);
    expect(config.sandbox.profile).toBe("develop-offline");
  });

  it("fails on invalid JSON", async () => {
    const path = await withConfigFile("{ not json");
    await expect(loadUserConfig(path)).rejects.toThrow("not valid JSON");
  });

  it("fails on unknown profiles in a file", async () => {
    const path = await withConfigFile(JSON.stringify({ sandbox: { profile: "nope" } }));
    await expect(loadUserConfig(path)).rejects.toThrow("Unknown sandbox profile");
  });

  it("rejects a config file beyond the byte limit without reading it fully", async () => {
    const directory = await mkdtemp(join(tmpdir(), "siralos-config-"));
    tempDirectories.push(directory);
    const path = join(directory, "huge.json");
    await writeFile(path, " ".repeat(1024 * 1024 + 1));
    await expect(loadUserConfig(path)).rejects.toThrow("byte limit");
  });

  it("rejects a config file that is not a regular file", async () => {
    const directory = await mkdtemp(join(tmpdir(), "siralos-config-"));
    tempDirectories.push(directory);
    const path = join(directory, "config.json");
    await writeFile(path, "{}");
    const { rm } = await import("node:fs/promises");
    await rm(path, { force: true });
    await mkdir(path);
    await expect(loadUserConfig(path)).rejects.toThrow("not a regular file");
  });
});

describe("parseUserConfig godot section", () => {
  it("defaults the godot section when missing", () => {
    expect(parseUserConfig({}).godot).toEqual(DEFAULT_USER_CONFIG.godot);
  });

  it("loads valid configured installations", () => {
    const config = parseUserConfig({
      godot: {
        activeInstallation: "primary",
        installations: {
          primary: {
            path: absoluteVersionedGodotPath,
            editionHint: "standard",
          },
        },
        discoverOnPath: true,
      },
    });
    expect(config.godot.activeInstallation).toBe("primary");
    expect(config.godot.installations["primary"]).toEqual({
      path: absoluteVersionedGodotPath,
      editionHint: "standard",
    });
    expect(config.godot.discoverOnPath).toBe(true);
  });

  it("defaults edition hints to unknown and discovery to true", () => {
    const config = parseUserConfig({
      godot: { installations: { primary: { path: absoluteGodotPath } } },
    });
    expect(config.godot.installations["primary"]?.editionHint).toBe("unknown");
    expect(config.godot.discoverOnPath).toBe(true);
  });

  it("rejects relative configured paths", () => {
    expect(() =>
      parseUserConfig({
        godot: { installations: { primary: { path: "godot.exe" } } },
      }),
    ).toThrow("path must be absolute");
  });

  it("rejects unknown godot section keys", () => {
    expect(() => parseUserConfig({ godot: { automaticDownload: true } })).toThrow(
      "Unknown Siralos godot configuration key",
    );
  });

  it("rejects unknown installation keys", () => {
    expect(() =>
      parseUserConfig({
        godot: { installations: { primary: { path: absoluteGodotPath, secret: "x" } } },
      }),
    ).toThrow("Unknown Godot installation key");
  });

  it("rejects unknown edition hints", () => {
    expect(() =>
      parseUserConfig({
        godot: {
          installations: { primary: { path: absoluteGodotPath, editionHint: "mono" } },
        },
      }),
    ).toThrow("Unknown Godot edition hint");
  });

  it("rejects empty installation ids", () => {
    expect(() =>
      parseUserConfig({ godot: { installations: { "": { path: absoluteGodotPath } } } }),
    ).toThrow("installation id");
  });

  it("rejects non-string active installations", () => {
    expect(() => parseUserConfig({ godot: { activeInstallation: 5 } })).toThrow(
      "godot.activeInstallation",
    );
  });

  it("rejects oversized installation maps", () => {
    const installations: Record<string, { path: string }> = {};
    for (let index = 0; index < 17; index += 1) {
      installations[`id-${index}`] = { path: absoluteGodotPath };
    }
    expect(() => parseUserConfig({ godot: { installations } })).toThrow("limited to 16");
  });

  it("rejects non-boolean discoverOnPath values", () => {
    expect(() => parseUserConfig({ godot: { discoverOnPath: "yes" } })).toThrow(
      "discoverOnPath must be a boolean",
    );
  });
});

describe("user-config schema", () => {
  it("matches the runtime validation enums", async () => {
    const { readFile } = await import("node:fs/promises");
    const schemaPath = new URL("../../../../schemas/user-config.schema.json", import.meta.url);
    const schema = JSON.parse(await readFile(schemaPath, "utf8")) as {
      properties: {
        sandbox: {
          properties: { profile: { enum: string[] }; backend: { enum: string[] } };
        };
        godot: {
          properties: {
            installations: {
              additionalProperties: {
                properties: { editionHint: { enum: string[] } };
              };
            };
          };
        };
      };
    };
    expect(schema.properties.sandbox.properties.profile.enum).toEqual([
      "inspect",
      "develop-offline",
    ]);
    expect(schema.properties.sandbox.properties.backend.enum).toEqual([
      "auto",
      "anthropic-runtime",
    ]);
    expect(
      schema.properties.godot.properties.installations.additionalProperties.properties.editionHint
        .enum,
    ).toEqual(["standard", "dotnet", "unknown"]);
  });
});
