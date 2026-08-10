import { describe, expect, it } from "vitest";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  DEVELOP_OFFLINE_PROFILE,
  INSPECT_PROFILE,
  createDefaultPolicy,
  getBuiltInProfile,
  type SandboxBackend,
  type SandboxBackendStatus,
} from "@solaris/core";
import { createCliDoctor, createCliDoctorSources } from "./doctor.js";
import { runDoctorCli } from "./doctor-cli.js";
import { readInstalledSolarisVersion, runningNodeMajor } from "./self-reference.js";

function fakeBackend(overrides: Partial<SandboxBackendStatus> = {}): SandboxBackend {
  return {
    id: "fake-backend",
    inspect(): Promise<SandboxBackendStatus> {
      return Promise.resolve({
        backendId: "fake-backend",
        state: "available",
        platform: "linux",
        version: "0.0.70",
        capabilities: {
          filesystemReadRestriction: true,
          filesystemWriteRestriction: true,
          networkRestriction: true,
          processTreeRestriction: true,
          violationReporting: true,
        },
        ...overrides,
      });
    },
    execute(): Promise<never> {
      return Promise.reject(new Error("fake backend never executes"));
    },
    close(): Promise<void> {
      return Promise.resolve();
    },
  };
}

describe("createCliDoctorSources (composition root wiring)", () => {
  it("reports a capability-deficient backend as a failure for a process-enabled profile (fail closed)", async () => {
    // Mirrors the authoritative execution gate (sandboxEnforcesBoundary):
    // a process-enabled profile requires ALL FOUR capabilities. A backend
    // that cannot restrict the network must fail the enforcement check,
    // never pass.
    const doctor = createCliDoctor({
      workspaceRoot: "/workspace",
      configPath: "/config.json",
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      sandbox: fakeBackend({
        capabilities: {
          filesystemReadRestriction: true,
          filesystemWriteRestriction: true,
          networkRestriction: false,
          processTreeRestriction: true,
          violationReporting: true,
        },
      }),
      provider: {
        id: "deterministic-fake",
        toolCalling: true,
        stream() {
          return (async function* () {})();
        },
      },
      godot: undefined as never,
      references: undefined as never,
      referenceConfigError: null,
      research: undefined as never,
      researchSources: [],
      tasks: undefined as never,
      taskSources: undefined as never,
      git: undefined as never,
      checkpoints: undefined as never,
      tools: [],
      mode: "generic",
    });
    const report = await doctor.inspect({ areas: ["sandbox"] });
    const enforcement = report.checks.find((check) => check.id === "sandbox.required_enforcement")!;
    expect(enforcement.status).toBe("fail");
    expect(JSON.stringify(enforcement.details)).toContain("networkRestriction");
  });

  it("skips required-enforcement for the inspect profile (no sandboxed execution is required)", async () => {
    const doctor = createCliDoctor({
      workspaceRoot: "/workspace",
      configPath: "/config.json",
      policy: createDefaultPolicy("inspect"),
      profile: INSPECT_PROFILE,
      sandbox: fakeBackend(),
      provider: {
        id: "deterministic-fake",
        toolCalling: true,
        stream() {
          return (async function* () {})();
        },
      },
      godot: undefined as never,
      references: undefined as never,
      referenceConfigError: null,
      research: undefined as never,
      researchSources: [],
      tasks: undefined as never,
      taskSources: undefined as never,
      git: undefined as never,
      checkpoints: undefined as never,
      tools: [],
      mode: "generic",
    });
    const report = await doctor.inspect({ areas: ["sandbox"] });
    const enforcement = report.checks.find((check) => check.id === "sandbox.required_enforcement")!;
    expect(enforcement.status).toBe("skip");
  });

  it("derives config diagnostics from the real file without ever exposing a secret value", async () => {
    const root = await mkdtemp(join(tmpdir(), "solaris-doctor-test-"));
    try {
      const secret = "sk-CLISECRETVALUE1234567890";
      const configPath = join(root, "config.json");
      await writeFile(configPath, JSON.stringify({ providerCredential: secret }), "utf8");
      const sources = createCliDoctorSources({
        workspaceRoot: root,
        configPath,
        policy: createDefaultPolicy("inspect"),
        profile: INSPECT_PROFILE,
        sandbox: fakeBackend(),
        provider: {
          id: "deterministic-fake",
          toolCalling: true,
          stream() {
            return (async function* () {})();
          },
        },
        godot: undefined as never,
        references: undefined as never,
        referenceConfigError: null,
        research: undefined as never,
        researchSources: [],
        tasks: undefined as never,
        taskSources: undefined as never,
        git: undefined as never,
        checkpoints: undefined as never,
        tools: [],
      });
      const configuration = await sources.configuration();
      expect(configuration.loaded).toBe(false);
      expect(configuration.validationErrors.length).toBeGreaterThan(0);
      const text = JSON.stringify(configuration);
      expect(text).not.toContain(secret);
      expect(text).not.toMatch(/sk-[A-Za-z0-9_-]{8,}/);
    } finally {
      const { rm } = await import("node:fs/promises");
      await rm(root, { recursive: true, force: true });
    }
  });

  it("reads the installed version from the CLI package metadata", () => {
    expect(readInstalledSolarisVersion()).toBe("0.0.0");
    expect(runningNodeMajor()).toBe(Number(process.versions.node.split(".")[0]));
  });
});

describe("runDoctorCli argv boundary", () => {
  it("exits 2 for an unknown doctor area before creating the application", async () => {
    const writes: string[] = [];
    const exitCode = await runDoctorCli(["--doctor", "bogus"], (text) => writes.push(text));
    expect(exitCode).toBe(2);
    expect(writes.join("")).toContain("Unknown doctor area: bogus");
  });

  it("exits 0 for a warnings-free area (runtime) and prints the report", async () => {
    const writes: string[] = [];
    const exitCode = await runDoctorCli(["--doctor", "runtime"], (text) => writes.push(text));
    expect(exitCode).toBe(0);
    const output = writes.join("");
    expect(output).toContain("Solaris Doctor");
    expect(output).toContain("runtime");
  });

  it("prints a valid JSON report with --json --report-safe", async () => {
    const writes: string[] = [];
    const exitCode = await runDoctorCli(
      ["--doctor", "runtime", "--json", "--report-safe"],
      (text) => writes.push(text),
    );
    expect(exitCode).toBe(0);
    const parsed = JSON.parse(writes.join("")) as {
      schemaVersion: number;
      runtime: { version: string };
    };
    expect(parsed.schemaVersion).toBe(1);
    expect(parsed.runtime.version).toBe("0.0.0");
  });
});

describe("installed profile wiring", () => {
  it("develop-offline is process-enabled (the enforcement gate applies)", () => {
    expect(DEVELOP_OFFLINE_PROFILE.process.enabled).toBe(true);
    expect(getBuiltInProfile("develop-offline").process.enabled).toBe(true);
  });
});
