import { mkdtemp, rm, writeFile } from "node:fs/promises";
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
    });
  });

  it("accepts partial sandbox sections with defaults", () => {
    expect(parseUserConfig({ sandbox: { profile: "develop-offline" } })).toEqual({
      sandbox: { profile: "develop-offline", backend: "auto" },
    });
    expect(parseUserConfig({ sandbox: {} })).toEqual(DEFAULT_USER_CONFIG);
  });

  it("rejects unknown profiles", () => {
    expect(() => parseUserConfig({ sandbox: { profile: "full-access" } })).toThrow(
      "Unknown sandbox profile",
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
  });
});
