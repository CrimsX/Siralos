import { mkdtemp, mkdir, readdir, rm, stat, utimes, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { completedResult, createFakeSandboxBackend } from "../../sandbox/fake-sandbox-backend.js";
import { createRunDirectoryProvider } from "../../process/run-directories.js";
import { hashFile, validateExecutable } from "../discovery/executable-validation.js";
import { installationFromIdentity } from "../discovery/path-discovery.js";
import { extractGodotApiDumpSummary } from "../api-dump/api-dump-summary.js";
import { PRIVATE_EXECUTABLE_COPY_NAME, stageVerifiedExecutableCopy } from "./executable-copy.js";
import { createGodotProbeRunner } from "./godot-probe-runner.js";
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

function availableStatus(): import("@solaris/core").SandboxBackendStatus {
  return {
    backendId: "fake-backend",
    state: "available",
    platform: "linux",
    version: "0.0.0-fake",
    capabilities: {
      filesystemReadRestriction: true,
      filesystemWriteRestriction: true,
      networkRestriction: true,
      processTreeRestriction: true,
      violationReporting: true,
    },
  };
}

/**
 * Backend that refuses to proceed unless the executed path is the verified
 * private copy inside the run directory (never the configured path) and its
 * complete SHA-256 equals the installation fingerprint.
 */
function copyVerifyingBackend(
  installation: GodotInstallation,
  versionText = "4.7.1.stable.official\n",
): { readonly backend: SandboxBackend; readonly requests: () => SandboxedProcessRequest[] } {
  const requests: SandboxedProcessRequest[] = [];
  const backend: SandboxBackend = {
    id: "copy-verifier",
    inspect() {
      return Promise.resolve(availableStatus());
    },
    async execute(request: SandboxedProcessRequest): Promise<SandboxedProcessResult> {
      requests.push(request);
      if (request.executable === installation.canonicalPath) {
        throw new Error("executed the mutable configured path");
      }
      if (!request.executable.startsWith(request.runDirectory ?? "\u0000")) {
        throw new Error("executed path is outside the run directory");
      }
      const bytes = await readFileBuffer(request.executable);
      const digest = createHash("sha256").update(bytes).digest("hex");
      if (digest !== installation.sha256) {
        throw new Error("executed bytes do not match the validated fingerprint");
      }
      return completedResult({ stdout: versionText });
    },
    close() {
      return Promise.resolve();
    },
  };
  return { backend, requests: () => requests };
}

async function readFileBuffer(path: string): Promise<Buffer> {
  const { readFile } = await import("node:fs/promises");
  return readFile(path);
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
    expect(request?.arguments).toEqual(["--version"]);
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
    // Abort once the probe request has reached the backend (after staging),
    // so the cancellation lands deterministically during execution.
    while (requests().length === 0) {
      await new Promise((resolve) => setTimeout(resolve, 1));
    }
    controller.abort();
    await expect(promise).rejects.toThrow("aborted");
    expect(requests().length).toBeGreaterThan(0);
    await assertNoRunDirectories(runsRoot);
  });

  it("fails a same-size replacement even with a restored mtime", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const metadata = await stat(installation.canonicalPath);
    await writeFile(installation.canonicalPath, "X".repeat(installation.sizeBytes));
    await utimes(installation.canonicalPath, metadata.atime, metadata.mtime);
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

  it("executes only a verified private copy equal to the reported fingerprint", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    // The backend verifies during execution that the executed path is the
    // private copy inside the run directory and that its complete SHA-256
    // equals the installation fingerprint; a mismatch fails the probe.
    const { backend, requests } = copyVerifyingBackend(installation);
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.probeVersion(installation);
    expect(probe.status).toBe("success");
    const request = requests()[0];
    expect(request?.executable.endsWith(PRIVATE_EXECUTABLE_COPY_NAME)).toBe(true);
    expect(request?.executable).not.toBe(installation.canonicalPath);
  });

  it("carries no workspace path anywhere in the probe request", async () => {
    const directory = await withTemp();
    const workspace = join(directory, "workspace");
    await mkdir(workspace, { recursive: true });
    const installation = await validInstallation(directory);
    const { backend, requests } = createFakeSandboxBackend({
      results: [completedResult({ stdout: "4.7.1.stable.official\n" })],
    });
    const { runner } = await probeDependencies(directory, backend);
    await runner.probeVersion(installation);
    const request = requests()[0];
    expect(request).toBeDefined();
    const serialized = JSON.stringify(request);
    expect(serialized).not.toContain(workspace);
    expect(serialized).not.toContain(installation.canonicalPath);
    expect(request?.explicitReadRoots).toEqual([]);
    for (const value of Object.values(request?.environment ?? {})) {
      expect(value).not.toContain(workspace);
    }
  });

  it("fails closed when the sandbox lacks host-read restriction", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend, requests } = createFakeSandboxBackend({
      status: {
        backendId: "fake",
        state: "available",
        platform: "linux",
        version: "0.0.0",
        capabilities: {
          filesystemReadRestriction: false,
          filesystemWriteRestriction: true,
          networkRestriction: true,
          processTreeRestriction: true,
          violationReporting: true,
        },
      },
    });
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.probeVersion(installation);
    expect(probe.status).toBe("failed");
    if (probe.status === "failed") {
      expect(probe.message).toMatch(/filesystem read restriction/);
    }
    expect(requests()).toEqual([]);
  });

  it("fails closed with a platform-accurate message when the sandbox is unavailable", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const { backend, requests } = createFakeSandboxBackend({
      status: {
        backendId: "fake",
        state: "unsupported",
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
      expect(probe.message).toMatch(/sandbox is unavailable/);
      if (process.platform === "win32") {
        expect(probe.message).toMatch(/host-read enforcement is not available on this platform/);
      }
    }
    expect(requests()).toEqual([]);
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
    expect(requests()[0]?.arguments).toEqual(["--dump-extension-api"]);
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

  it("never reports success for a nonzero exit even with a valid-looking dump", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const backend: SandboxBackend = {
      ...dumpWritingBackend(validDump()),
      async execute(request: SandboxedProcessRequest): Promise<SandboxedProcessResult> {
        await writeFile(join(request.workingDirectory, "extension_api.json"), validDump());
        return completedResult({ exitCode: 1 });
      },
    };
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.dumpExtensionApi(installation);
    expect(probe.status).toBe("degraded");
    if (probe.status === "degraded") {
      expect(probe.message).toMatch(/exited with code 1/);
    }
  });

  it("fingerprints the exact raw dump bytes including multibyte UTF-8", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const dump = JSON.stringify({
      header: { version_full_name: "4.7.1.stable.official", hash: "abc123" },
      builtin_classes: [],
      global_enums: [],
      utility_functions: [],
      classes: [],
      configurations: { format_version: 5 },
      notes: "日本語 emoji 🎮 and lone \ud800 surrogate handling",
    });
    const { runner } = await probeDependencies(directory, dumpWritingBackend(dump));
    const probe = await runner.dumpExtensionApi(installation);
    expect(probe.status).toBe("success");
    if (probe.status === "success") {
      expect(probe.summary.sha256).toBe(
        createHash("sha256").update(Buffer.from(dump, "utf8")).digest("hex"),
      );
      expect(probe.summary.fileSizeBytes).toBe(Buffer.byteLength(dump, "utf8"));
    }
  });

  it("executes the dump probe from the verified private copy", async () => {
    const directory = await withTemp();
    const installation = await validInstallation(directory);
    const requests: SandboxedProcessRequest[] = [];
    const backend: SandboxBackend = {
      ...dumpWritingBackend(validDump()),
      async execute(request: SandboxedProcessRequest): Promise<SandboxedProcessResult> {
        requests.push(request);
        await writeFile(join(request.workingDirectory, "extension_api.json"), validDump());
        return completedResult({});
      },
    };
    const { runner } = await probeDependencies(directory, backend);
    const probe = await runner.dumpExtensionApi(installation);
    expect(probe.status).toBe("success");
    const request = requests[0];
    expect(request?.executable.endsWith(PRIVATE_EXECUTABLE_COPY_NAME)).toBe(true);
    expect(request?.executable).not.toBe(installation.canonicalPath);
    expect(request?.explicitReadRoots).toEqual([]);
  });
});

describe("extractGodotApiDumpSummary", () => {
  it("hashes the raw buffer and reports the raw byte length", () => {
    const raw = Buffer.from(
      JSON.stringify({
        header: { version_full_name: "4.7.1.stable.official", hash: "abc" },
        builtin_classes: [],
        global_enums: [],
        utility_functions: [],
        classes: [],
        configurations: { format_version: 5 },
        notes: "日本語 🎮",
      }),
      "utf8",
    );
    const extraction = extractGodotApiDumpSummary(raw);
    expect(extraction.ok).toBe(true);
    if (extraction.ok) {
      expect(extraction.summary.sha256).toBe(createHash("sha256").update(raw).digest("hex"));
      expect(extraction.summary.fileSizeBytes).toBe(raw.length);
      expect(extraction.summary.fileSizeBytes).not.toBe(raw.toString("utf8").length);
    }
  });
});

describe("stageVerifiedExecutableCopy", () => {
  it("stages a verified copy whose hash equals the expected fingerprint", async () => {
    const directory = await withTemp();
    const source = join(directory, "godot.exe");
    await writeFile(source, "executable bytes");
    const expectedSha256 = createHash("sha256").update("executable bytes").digest("hex");
    const runRoot = join(directory, "run");
    await mkdir(runRoot);
    const staged = await stageVerifiedExecutableCopy({
      sourcePath: source,
      runRoot,
      expectedSha256,
      maxBytes: 1024 * 1024,
    });
    expect(staged.ok).toBe(true);
    if (staged.ok) {
      expect(staged.copyPath).toBe(join(runRoot, PRIVATE_EXECUTABLE_COPY_NAME));
      expect(await hashFile(staged.copyPath)).toBe(expectedSha256);
      if (process.platform !== "win32") {
        const { lstat } = await import("node:fs/promises");
        const mode = (await lstat(staged.copyPath)).mode;
        expect(mode & 0o111).not.toBe(0);
      }
    }
  });

  it("fails closed when the copy hash does not match the expected fingerprint", async () => {
    const directory = await withTemp();
    const source = join(directory, "godot.exe");
    await writeFile(source, "executable bytes");
    const runRoot = join(directory, "run");
    await mkdir(runRoot);
    const staged = await stageVerifiedExecutableCopy({
      sourcePath: source,
      runRoot,
      expectedSha256: "0".repeat(64),
      maxBytes: 1024 * 1024,
    });
    expect(staged.ok).toBe(false);
    if (!staged.ok) {
      expect(staged.error).toMatch(/does not match the validated fingerprint/);
    }
    const entries = await readdir(runRoot);
    expect(entries).toEqual([]);
  });

  it("fails closed when the run root is not a real directory", async () => {
    const directory = await withTemp();
    const source = join(directory, "godot.exe");
    await writeFile(source, "executable bytes");
    const runRoot = join(directory, "not-a-directory");
    await writeFile(runRoot, "i am a file");
    const staged = await stageVerifiedExecutableCopy({
      sourcePath: source,
      runRoot,
      expectedSha256: "0".repeat(64),
      maxBytes: 1024 * 1024,
    });
    expect(staged.ok).toBe(false);
  });

  it("fails closed when the source is missing", async () => {
    const directory = await withTemp();
    const runRoot = join(directory, "run");
    await mkdir(runRoot);
    const staged = await stageVerifiedExecutableCopy({
      sourcePath: join(directory, "missing.exe"),
      runRoot,
      expectedSha256: "0".repeat(64),
      maxBytes: 1024 * 1024,
    });
    expect(staged.ok).toBe(false);
    if (!staged.ok) {
      expect(staged.error).toMatch(/could not be re-verified/);
    }
  });

  it("fails closed when the source is replaced by a symlink", async () => {
    const directory = await withTemp();
    const real = join(directory, "real.exe");
    const source = join(directory, "linked.exe");
    await writeFile(real, "real bytes");
    try {
      const { symlink } = await import("node:fs/promises");
      await symlink(real, source);
    } catch {
      // symlinks unsupported (e.g. restricted Windows) - skip
      return;
    }
    const runRoot = join(directory, "run");
    await mkdir(runRoot);
    const staged = await stageVerifiedExecutableCopy({
      sourcePath: source,
      runRoot,
      expectedSha256: "0".repeat(64),
      maxBytes: 1024 * 1024,
    });
    expect(staged.ok).toBe(false);
    if (!staged.ok) {
      expect(staged.error).toMatch(/could not be re-verified/);
    }
  });

  it("fails closed when the copy exceeds the size bound", async () => {
    const directory = await withTemp();
    const source = join(directory, "godot.exe");
    await writeFile(source, "123456789");
    const runRoot = join(directory, "run");
    await mkdir(runRoot);
    const staged = await stageVerifiedExecutableCopy({
      sourcePath: source,
      runRoot,
      expectedSha256: "0".repeat(64),
      maxBytes: 5,
    });
    expect(staged.ok).toBe(false);
    if (!staged.ok) {
      expect(staged.error).toMatch(/size limit/);
    }
    const entries = await readdir(runRoot);
    expect(entries).toEqual([]);
  });

  it("stages a macOS bundle executable from the private copy without bundle access", async () => {
    const directory = await withTemp();
    const bundle = join(directory, "Godot.app");
    const macos = join(bundle, "Contents", "MacOS");
    await mkdir(macos, { recursive: true });
    const executable = join(macos, "Godot");
    await writeFile(executable, "bundle executable bytes");
    const result = await validateExecutable({
      path: executable,
      workspaceRoot: join(directory, "workspace"),
    });
    expect(result.ok).toBe(true);
    if (!result.ok) {
      return;
    }
    expect(result.identity.bundlePath).toBe(bundle);
    const runRoot = join(directory, "run");
    await mkdir(runRoot);
    const staged = await stageVerifiedExecutableCopy({
      sourcePath: result.identity.canonicalPath,
      runRoot,
      expectedSha256: result.identity.sha256,
      maxBytes: 1024 * 1024,
    });
    expect(staged.ok).toBe(true);
    // Removing the entire bundle after staging must not affect the verified
    // copy: the probe executes only the private copy.
    await rm(bundle, { recursive: true, force: true });
    if (staged.ok) {
      expect(await hashFile(staged.copyPath)).toBe(result.identity.sha256);
    }
  });
});

describe("probe argument discipline", () => {
  it("passes exactly one fixed probe argument to the sandbox", async () => {
    const executableRoot = await withTemp();
    const workspaceRoot = await withTemp();
    const executable = join(executableRoot, "godot-test");
    await writeFile(executable, "#!/bin/sh\necho fixture\n");
    await chmodExecutable(executable);
    const validated = await validateExecutable({ path: executable, workspaceRoot });
    expect(validated.ok).toBe(true);
    if (!validated.ok) {
      return;
    }
    const installation = installationFromIdentity(
      "test",
      "user-config",
      "user config",
      validated.identity,
      "standard",
    );
    const backend = createFakeSandboxBackend({
      status: {
        backendId: "fake-backend",
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
      },
    });
    const runDirectories = createRunDirectoryProvider({
      workspaceRoot,
      runsRoot: join(executableRoot, "runs"),
    });
    const runner = createGodotProbeRunner({
      backend: backend.backend,
      runDirectories,
      parentEnvironment: { HOME: "x", TEMP: "x" },
    });
    await runner.probeVersion(installation);
    await runner.probeHelp(installation);
    await runner.dumpExtensionApi(installation);
    const requests = backend.requests();
    expect(requests).toHaveLength(3);
    expect(requests[0]?.arguments).toEqual(["--version"]);
    expect(requests[1]?.arguments).toEqual(["--help"]);
    expect(requests[2]?.arguments).toEqual(["--dump-extension-api"]);
  });
});

async function chmodExecutable(path: string): Promise<void> {
  const { chmod } = await import("node:fs/promises");
  await chmod(path, 0o755).catch(() => undefined);
}
