import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { DEFAULT_USER_CONFIG, loadUserConfig, parseUserConfig } from "./user-config.js";

const tempDirectories: string[] = [];

async function withConfigFile(content: string): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "solaris-config-"));
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
    });
  });

  it("accepts partial sandbox sections with defaults", () => {
    expect(parseUserConfig({ sandbox: { profile: "develop-offline" } })).toEqual({
      sandbox: { profile: "develop-offline", backend: "auto" },
      godot: DEFAULT_USER_CONFIG.godot,
      quality: DEFAULT_USER_CONFIG.quality,
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
      "Unknown Solaris quality configuration key",
    );
    expect(() => parseUserConfig({ quality: "reviewer" })).toThrow(
      'Solaris configuration section "quality" must be a JSON object',
    );
  });

  it("rejects unknown backends", () => {
    expect(() => parseUserConfig({ sandbox: { backend: "docker" } })).toThrow(
      "Unknown sandbox backend",
    );
  });

  it("rejects unknown top-level sections", () => {
    expect(() => parseUserConfig({ permissions: {} })).toThrow(
      "Unknown Solaris configuration section",
    );
  });

  it("rejects unknown sandbox keys", () => {
    expect(() => parseUserConfig({ sandbox: { networkAllowlist: [] } })).toThrow(
      "Unknown Solaris sandbox configuration key",
    );
  });

  it("rejects non-object configuration", () => {
    expect(() => parseUserConfig("config")).toThrow();
    expect(() => parseUserConfig([1, 2])).toThrow();
  });

  it("cannot contain credentials", () => {
    expect(() => parseUserConfig({ sandbox: { apiKey: "secret" } })).toThrow(
      "Unknown Solaris sandbox configuration key",
    );
  });
});

describe("loadUserConfig", () => {
  it("returns defaults when no config file exists", async () => {
    const directory = await mkdtemp(join(tmpdir(), "solaris-config-"));
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
    const directory = await mkdtemp(join(tmpdir(), "solaris-config-"));
    tempDirectories.push(directory);
    const path = join(directory, "huge.json");
    await writeFile(path, " ".repeat(1024 * 1024 + 1));
    await expect(loadUserConfig(path)).rejects.toThrow("byte limit");
  });

  it("rejects a config file that is not a regular file", async () => {
    const directory = await mkdtemp(join(tmpdir(), "solaris-config-"));
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
            path: "C:\\Tools\\Godot\\Godot_v4.7.1-stable_win64.exe",
            editionHint: "standard",
          },
        },
        discoverOnPath: true,
      },
    });
    expect(config.godot.activeInstallation).toBe("primary");
    expect(config.godot.installations["primary"]).toEqual({
      path: "C:\\Tools\\Godot\\Godot_v4.7.1-stable_win64.exe",
      editionHint: "standard",
    });
    expect(config.godot.discoverOnPath).toBe(true);
  });

  it("defaults edition hints to unknown and discovery to true", () => {
    const config = parseUserConfig({
      godot: { installations: { primary: { path: "C:\\godot.exe" } } },
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
      "Unknown Solaris godot configuration key",
    );
  });

  it("rejects unknown installation keys", () => {
    expect(() =>
      parseUserConfig({
        godot: { installations: { primary: { path: "C:\\godot.exe", secret: "x" } } },
      }),
    ).toThrow("Unknown Godot installation key");
  });

  it("rejects unknown edition hints", () => {
    expect(() =>
      parseUserConfig({
        godot: { installations: { primary: { path: "C:\\godot.exe", editionHint: "mono" } } },
      }),
    ).toThrow("Unknown Godot edition hint");
  });

  it("rejects empty installation ids", () => {
    expect(() =>
      parseUserConfig({ godot: { installations: { "": { path: "C:\\godot.exe" } } } }),
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
      installations[`id-${index}`] = { path: "C:\\godot.exe" };
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
