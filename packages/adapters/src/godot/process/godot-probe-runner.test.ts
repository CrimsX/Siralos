import { mkdtemp, mkdir, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { completedResult, createFakeSandboxBackend } from "../../sandbox/fake-sandbox-backend.js";
import { createRunDirectoryProvider } from "../../process/run-directories.js";
import { validateExecutable } from "../discovery/executable-validation.js";
import { installationFromIdentity } from "../discovery/path-discovery.js";
import {
  createGodotProbeRunner,
  GODOT_API_DUMP_ARGUMENTS,
  GODOT_HELP_ARGUMENTS,
  GODOT_VERSION_ARGUMENTS,
} from "./godot-probe-runner.js";
import type { GodotInstallation } from "@solaris/core";
import type {
  SandboxBackend,
  SandboxedProcessRequest,
  SandboxedProcessResult,
} from "@solaris/core";

const tempDirectories: string[] = [];

async function withTemp(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "solaris-godot-probe-"));
  tempDirectories.push(directory);
  return directory;
}

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

async function validInstallation(directory: string): Promise<GodotInstallation> {
  await mkdir(directory, { recursive: true });
  const path = join(directory, "godot.exe");
  await writeFile(path, "fake godot executable");
  const result = await validateExecutable({ path, workspaceRoot: join(directory, "workspace") });
  if (!result.ok) {
    throw new Error("fixture executable did not validate");
  }
  return installationFromIdentity(
    "primary",
    "user-config",
    "user config",
    result.identity,
    "standard",
  );
}

function dumpWritingBackend(dumpContent: string): SandboxBackend {
  return {
    id: "dump-writer",
    inspect() {
      return Promise.resolve({
        backendId: "dump-writer",
        state: "available",
        platform: "linux",
        version: "0.0.0",
        capabilities: {
          filesystemReadRestriction: true,
          filesystemWriteRestriction: true,
          networkRestriction: true,
          processTreeRestriction: true,
          violationReporting: true,
        },
      });
    },
    async execute(request: SandboxedProcessRequest): Promise<SandboxedProcessResult> {
      await writeFile(join(request.workingDirectory, "extension_api.json"), dumpContent);
      return completedResult({});
    },
    close() {
      return Promise.resolve();
    },
  };
}

function validDump(): string {
  return JSON.stringify({
    header: {
      version_major: 4,
      version_minor: 7,
      version_patch: 1,
      version_status: "stable",
      version_build: "official",
      version_full_name: "4.7.1.stable.official",
      hash: "abc123",
    },
    builtin_class_sizes: [],
    builtin_class_members: [],
    global_constants: [],
    global_enums: [{}],
    utility_functions: [{}, {}],
    builtin_classes: [{}, {}],
    classes: [{}, {}, {}],
    singletons: [],
    native_structures: [],
    configurations: { format_version: 5 },
  });
}

async function probeDependencies(directory: string, backend: SandboxBackend) {
  const runsRoot = join(directory, "runs");
  await mkdir(runsRoot, { recursive: true });
  const runner = createGodotProbeRunner({
    backend,
    runDirectories: createRunDirectoryProvider({
      workspaceRoot: join(directory, "workspace"),
      runsRoot,
    }),
    parentEnvironment: { PATH: process.env["PATH"] ?? "" },
  });
  return { runner, runsRoot };
}

async function assertNoRunDirectories(runsRoot: string): Promise<void> {
  const fingerprints = await readdir(runsRoot);
  for (const fingerprint of fingerprints) {
    const entries = await readdir(join(runsRoot, fingerprint));
    expect(entries).toEqual([]);
  }
}

describe("probeVersion", () => {
  it("parses a stable version from stdout", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend, requests } = createFakeSandboxBackend({
      results: [completedResult({ stdout: "4.7.1.stable.official\n" })],
    });
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.probeVersion(installation);
    expect(probe.status).toBe("success");
    if (probe.status === "success") {
      expect(probe.version).toMatchObject({ major: 4, minor: 7, patch: 1, status: "stable" });
    }
    expect(requests()[0]?.arguments).toEqual(["--version"]);
  });

  it("passes only fixed arguments with no project path", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend, requests } = createFakeSandboxBackend({
      results: [completedResult({ stdout: "4.7.1.stable.official\n" })],
    });
    const { runner } = await probeDependencies(directory, backend);
    await runner.probeVersion(installation);
    const request = requests()[0];
    expect(request?.arguments).toEqual(GODOT_VERSION_ARGUMENTS);
    expect(request?.workingDirectory).not.toContain("workspace");
    expect(request?.profile.id).toBe("godot-probe-offline");
  });

  it("rejects empty output", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend } = createFakeSandboxBackend({ results: [completedResult({ stdout: "" })] });
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.probeVersion(installation);
    expect(probe.status).toBe("failed");
  });

  it("rejects non-Godot output", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend } = createFakeSandboxBackend({
      results: [completedResult({ stdout: "hello world\n" })],
    });
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.probeVersion(installation);
    expect(probe.status).toBe("failed");
  });

  it("fails on non-zero exit codes", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend } = createFakeSandboxBackend({
      results: [completedResult({ exitCode: 1, stdout: "4.7.1.stable.official\n" })],
    });
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.probeVersion(installation);
    expect(probe.status).toBe("failed");
  });

  it("fails closed when the sandbox is unavailable and never executes", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend, requests } = createFakeSandboxBackend({
      status: {
        backendId: "fake",
        state: "setup-required",
        platform: "windows",
        version: "0.0.0",
        capabilities: {
          filesystemReadRestriction: false,
          filesystemWriteRestriction: false,
          networkRestriction: false,
          processTreeRestriction: false,
          violationReporting: false,
        },
      },
    });
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.probeVersion(installation);
    expect(probe.status).toBe("failed");
    if (probe.status === "failed") {
      expect(probe.message).toMatch(/fail closed/);
    }
    expect(requests()).toEqual([]);
  });

  it("fails when the executable identity changed after validation", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    await new Promise((resolve) => setTimeout(resolve, 5));
    await writeFile(installation.canonicalPath, "modified executable");
    const { backend, requests } = createFakeSandboxBackend({
      results: [completedResult({ stdout: "4.7.1.stable.official\n" })],
    });
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.probeVersion(installation);
    expect(probe.status).toBe("failed");
    if (probe.status === "failed") {
      expect(probe.message).toMatch(/rediscovery/);
    }
    expect(requests()).toEqual([]);
  });

  it("propagates cancellation and cleans the run directory", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend, requests } = createFakeSandboxBackend({
      results: [completedResult({ status: "cancelled" })],
    });
    const { runner, runsRoot } = await probeDependencies(directory, backend);
    const controller = new AbortController();
    const promise = runner.probeVersion(installation, controller.signal);
    controller.abort();
    await expect(promise).rejects.toThrow("aborted");
    expect(requests().length).toBeGreaterThan(0);
    await assertNoRunDirectories(runsRoot);
  });
});

describe("probeHelp", () => {
  const helpText = [
    "--help  List of command line options.",
    "--editor  Starts the editor.",
    "--path <directory>  Path to a project.",
    "--headless  Run without a window.",
    "--lsp-port <port>  LSP port.",
    "--dump-extension-api  Generate extension_api.json.",
    "--custom-thing  Unknown option.",
  ].join("\n");

  it("extracts exact option capabilities", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend } = createFakeSandboxBackend({
      results: [completedResult({ stdout: helpText })],
    });
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.probeHelp(installation);
    expect(probe.status).toBe("success");
    if (probe.status === "success") {
      expect(probe.capabilities.editor).toBe(true);
      expect(probe.capabilities.projectPath).toBe(true);
      expect(probe.capabilities.headless).toBe(true);
      expect(probe.capabilities.lsp).toBe(true);
      expect(probe.capabilities.extensionApiDump).toBe(true);
      expect(probe.capabilities.import).toBe(false);
      expect(probe.unknownOptionCount).toBe(1);
    }
  });

  it("does not match substrings", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend } = createFakeSandboxBackend({
      results: [
        completedResult({
          stdout: "--editing-mode is not the same as the editor\n--headlessism is not headless\n",
        }),
      ],
    });
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.probeHelp(installation);
    expect(probe.status).toBe("success");
    if (probe.status === "success") {
      expect(probe.capabilities.editor).toBe(false);
      expect(probe.capabilities.headless).toBe(false);
    }
  });

  it("degrades on non-zero exit with output", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend } = createFakeSandboxBackend({
      results: [completedResult({ exitCode: 2, stdout: "--editor\n" })],
    });
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.probeHelp(installation);
    expect(probe.status).toBe("degraded");
  });

  it("fails without any output", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend } = createFakeSandboxBackend({
      results: [completedResult({ stdout: "" })],
    });
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.probeHelp(installation);
    expect(probe.status).toBe("failed");
  });
});

describe("dumpExtensionApi", () => {
  it("produces a bounded summary from a valid dump and cleans up", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { runner, runsRoot } = await probeDependencies(
      directory,
      dumpWritingBackend(validDump()),
    );
    const probe = await runner.dumpExtensionApi(installation);
    expect(probe.status).toBe("success");
    if (probe.status === "success") {
      expect(probe.summary.headerVersion).toBe("4.7.1.stable.official");
      expect(probe.summary.apiHash).toBe("abc123");
      expect(probe.summary.classCount).toBe(3);
      expect(probe.summary.builtinClassCount).toBe(2);
      expect(probe.summary.globalEnumCount).toBe(1);
      expect(probe.summary.utilityFunctionCount).toBe(2);
      expect(probe.summary.configurationVersion).toBe(5);
      expect(probe.summary.sha256).toMatch(/^[0-9a-f]{64}$/);
      expect(probe.summary.fileSizeBytes).toBeGreaterThan(0);
    }
    await assertNoRunDirectories(runsRoot);
  });

  it("degrades when the dump is missing", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend } = createFakeSandboxBackend({ results: [completedResult({})] });
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.dumpExtensionApi(installation);
    expect(probe.status).toBe("degraded");
  });

  it("degrades on invalid JSON", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { runner } = await probeDependencies(directory, dumpWritingBackend("{ not json"));
    const probe = await runner.dumpExtensionApi(installation);
    expect(probe.status).toBe("degraded");
  });

  it("ignores unexpected output files when the dump exists", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const backend: SandboxBackend = {
      ...dumpWritingBackend(validDump()),
      async execute(request: SandboxedProcessRequest): Promise<SandboxedProcessResult> {
        await writeFile(join(request.workingDirectory, "extension_api.json"), validDump());
        await writeFile(join(request.workingDirectory, "surprise.txt"), "ignored");
        return completedResult({});
      },
    };
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.dumpExtensionApi(installation);
    expect(probe.status).toBe("success");
  });

  it("runs only --dump-extension-api", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend, requests } = createFakeSandboxBackend({
      results: [completedResult({})],
    });
    const { runner } = await probeDependencies(directory, backend);
    await runner.dumpExtensionApi(installation);
    expect(requests()[0]?.arguments).toEqual(GODOT_API_DUMP_ARGUMENTS);
    expect(requests()[0]?.arguments).not.toContain("--dump-extension-api-with-docs");
    expect(requests()[0]?.arguments).not.toContain("--doctool");
  });

  it("never touches the workspace", async () => {
    const directory = await withTemp();
    const workspace = join(directory, "workspace");
    await mkdir(workspace);
    await writeFile(join(workspace, "project.godot"), "config_version=5\n");
    const installation = await validInstallation(directory);
    const { runner } = await probeDependencies(directory, dumpWritingBackend(validDump()));
    const probe = await runner.dumpExtensionApi(installation);
    expect(probe.status).toBe("success");
    const entries = await readdir(workspace);
    expect(entries).toEqual(["project.godot"]);
  });
});

describe("probe argument discipline", () => {
  it("passes exactly --version and --help and the dump flag", async () => {
    expect(GODOT_VERSION_ARGUMENTS).toEqual(["--version"]);
    expect(GODOT_HELP_ARGUMENTS).toEqual(["--help"]);
    expect(GODOT_API_DUMP_ARGUMENTS).toEqual(["--dump-extension-api"]);
  });
});
