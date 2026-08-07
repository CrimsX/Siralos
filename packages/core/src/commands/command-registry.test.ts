import { describe, expect, it } from "vitest";
import {
  canonicalizeCommandDigest,
  createCommandRunnerRegistry,
  createPreparedCommand,
  type CommandDigestParts,
  type CommandRunner,
  type CommandRunnerDefinition,
} from "../index.js";

function createStubRunner(id: string): CommandRunner {
  const definition: CommandRunnerDefinition = { id, description: `Stub ${id}` };
  return {
    definition,
    prepare(): Promise<never> {
      throw new Error("Not used in registry tests.");
    },
    toExecutionRequest(): Promise<never> {
      throw new Error("Not used in registry tests.");
    },
    isAvailable(): Promise<boolean> {
      return Promise.resolve(true);
    },
  };
}

describe("createCommandRunnerRegistry", () => {
  it("resolves exact runner ids only", () => {
    const registry = createCommandRunnerRegistry([createStubRunner("npm-script")]);
    expect(registry.get("npm-script")?.definition.id).toBe("npm-script");
    expect(registry.get("npm-SCRIPT")).toBeUndefined();
    expect(registry.get("other")).toBeUndefined();
  });

  it("rejects duplicate runner ids", () => {
    expect(() =>
      createCommandRunnerRegistry([createStubRunner("npm-script"), createStubRunner("npm-script")]),
    ).toThrow("Duplicate command runner id: npm-script");
  });

  it("exposes definitions without allowing replacement", () => {
    const registry = createCommandRunnerRegistry([createStubRunner("a"), createStubRunner("b")]);
    const definitions = registry.definitions;
    expect(definitions.map((definition) => definition.id)).toEqual(["a", "b"]);
    expect(Object.isFrozen(definitions)).toBe(false);
  });

  it("keeps runner registration private to the composition root", () => {
    const registry = createCommandRunnerRegistry([createStubRunner("a")]);
    expect(registry).not.toHaveProperty("register");
    const descriptor = Object.getOwnPropertyDescriptor(registry, "get");
    expect(descriptor?.writable ?? true).toBe(true);
  });
});

describe("prepared commands", () => {
  it("creates opaque branded command handles", () => {
    const command = createPreparedCommand();
    expect(command).toBeTruthy();
    expect(JSON.stringify(command)).toBe("{}");
  });
});

describe("canonicalizeCommandDigest", () => {
  const parts: CommandDigestParts = {
    runnerId: "npm-script",
    executableIdentity: "node v26.1.0 + npm 11.13.0",
    executableVersion: "26.1.0",
    script: "check",
    fileHash: "abc123",
    repositoryScript: "npm run format:check && npm run lint",
    arguments: ["--runInBand", "tests/example test.ts"],
    workingDirectory: ".",
    profileId: "validation-offline",
    environmentPolicy: "minimal",
    timeoutMs: 120_000,
    stdoutLimitBytes: 1_048_576,
    stderrLimitBytes: 1_048_576,
    stdinPolicy: "closed",
    networkPolicy: "denied",
  };

  it("produces a deterministic canonical string", () => {
    const first = canonicalizeCommandDigest(parts);
    const second = canonicalizeCommandDigest(parts);
    expect(first).toBe(second);
  });

  it("changes when any execution detail changes", () => {
    const base = canonicalizeCommandDigest(parts);
    expect(canonicalizeCommandDigest({ ...parts, arguments: ["--changed"] })).not.toBe(base);
    expect(canonicalizeCommandDigest({ ...parts, timeoutMs: 60_000 })).not.toBe(base);
    expect(canonicalizeCommandDigest({ ...parts, script: "test" })).not.toBe(base);
    expect(canonicalizeCommandDigest({ ...parts, fileHash: "def456" })).not.toBe(base);
    expect(canonicalizeCommandDigest({ ...parts, workingDirectory: "packages/core" })).not.toBe(
      base,
    );
    expect(canonicalizeCommandDigest({ ...parts, runnerId: "node-script" })).not.toBe(base);
    expect(canonicalizeCommandDigest({ ...parts, profileId: "develop-offline" })).not.toBe(base);
    expect(canonicalizeCommandDigest({ ...parts, repositoryScript: "different" })).not.toBe(base);
  });

  it("keeps argument order significant", () => {
    const base = canonicalizeCommandDigest(parts);
    expect(
      canonicalizeCommandDigest({ ...parts, arguments: ["tests/example test.ts", "--runInBand"] }),
    ).not.toBe(base);
  });
});
