import { mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createServer, type Server } from "node:net";
import { tmpdir } from "node:os";
import { basename, dirname, join } from "node:path";
import type {
  SandboxBackend,
  SandboxProfile,
  SandboxedProcessRequest,
  SandboxedProcessResult,
} from "@solaris/core";
import { COMMAND_LIMITS, VALIDATION_OFFLINE_PROFILE } from "@solaris/core";
import { buildCommandEnvironment } from "../../environment/command-environment.js";
import { createRunDirectoryProvider, type CommandRunPaths } from "../../process/run-directories.js";
import { resolveNpmCli, type NpmCliResolution } from "../../process/trusted-executables.js";

export interface ConformanceProbeResult {
  readonly probeId: string;
  readonly description: string;
  readonly outcome: "passed" | "failed" | "skipped";
  readonly detail: string;
}

export interface ConformanceReport {
  readonly backendId: string;
  readonly platform: string;
  readonly profileId: string;
  readonly results: readonly ConformanceProbeResult[];
  readonly passed: number;
  readonly failed: number;
  readonly skipped: number;
}

export interface ConformanceOptions {
  readonly workspaceRoot: string;
  readonly profile: SandboxProfile;
  readonly parentEnvironment: Readonly<Record<string, string>>;
}

const FIXTURE_FILE_NAME = "solaris-conformance-fixture.cjs";
const PROBE_TARGET_FILE_NAME = "probe-target.txt";
const OUTSIDE_SECRET_FILE_NAME = "secret.txt";
const NPM_PROBE_DIRECTORY = "npm-probe";
const NPM_PROBE_FILE_NAME = "solaris-npm-probe.cjs";
const RUNS_ROOT = join(tmpdir(), "solaris-conformance-runs");
const HEARTBEAT_FILE_NAME = "solaris-heartbeat.txt";
const HEARTBEAT_GRACE_MS = 1500;

interface ProbeContext {
  readonly workspaceRoot: string;
  readonly outsideDirectory: string;
  readonly secretPath: string;
  readonly npmCli: NpmCliResolution;
  loopbackPort: number;
  /** Current probe's sandbox-private run temp; set before each execution. */
  runTemp: string;
}

interface Probe {
  readonly id: string;
  readonly description: string;
  readonly profile?: SandboxProfile;
  readonly profileOverrides?: Partial<SandboxProfile["process"]>;
  readonly needsLoopbackServer?: boolean;
  readonly cancelAfterMs?: number;
  readonly timeoutMs?: number;
  readonly buildRequest: (
    context: ProbeContext,
    runPaths: CommandRunPaths,
  ) => Promise<SandboxedProcessRequest>;
  readonly check: (
    result: SandboxedProcessResult,
    context: ProbeContext,
  ) => Promise<boolean> | boolean;
  /** Runs after the probe executes, before the next probe starts. */
  readonly cleanupAfter?: (context: ProbeContext) => Promise<void>;
}

export async function runSandboxConformance(
  backend: SandboxBackend,
  options: ConformanceOptions,
): Promise<ConformanceReport> {
  const fixturePath = join(options.workspaceRoot, FIXTURE_FILE_NAME);
  await writeFile(fixturePath, FIXTURE_SOURCE);
  await writeFile(join(options.workspaceRoot, PROBE_TARGET_FILE_NAME), "probe-target\n");
  const npmProbeDirectory = join(options.workspaceRoot, NPM_PROBE_DIRECTORY);
  await mkdir(npmProbeDirectory, { recursive: true });
  await writeFile(join(npmProbeDirectory, "package.json"), NPM_PACKAGE_JSON);
  await writeFile(join(npmProbeDirectory, NPM_PROBE_FILE_NAME), NPM_PROBE_SOURCE);
  const outsideDirectory = join(
    dirname(options.workspaceRoot),
    `${basename(options.workspaceRoot)}-outside`,
  );
  await mkdir(outsideDirectory, { recursive: true });
  const secretPath = join(outsideDirectory, OUTSIDE_SECRET_FILE_NAME);
  await writeFile(secretPath, "PROBE-SECRET-DO-NOT-USE\n");

  const npmCli = await resolveNpmCli();
  const runDirectories = createRunDirectoryProvider({
    workspaceRoot: options.workspaceRoot,
    runsRoot: RUNS_ROOT,
  });
  const probeContext: ProbeContext = {
    workspaceRoot: options.workspaceRoot,
    outsideDirectory,
    secretPath,
    loopbackPort: 0,
    npmCli,
    runTemp: "",
  };

  const nodeRequest = (
    probeId: string,
    profile: SandboxProfile,
    runPaths: CommandRunPaths,
  ): Promise<SandboxedProcessRequest> =>
    Promise.resolve({
      executable: process.execPath,
      arguments: [
        fixturePath,
        probeId,
        probeContext.outsideDirectory,
        probeContext.secretPath,
        String(probeContext.loopbackPort),
        join(runPaths.temp, HEARTBEAT_FILE_NAME),
      ],
      workingDirectory: options.workspaceRoot,
      profile,
      environment: buildCommandEnvironment(
        options.parentEnvironment,
        {
          home: runPaths.home,
          temp: runPaths.temp,
          npmCache: runPaths.npmCache,
          npmUserConfig: runPaths.npmUserConfig,
        },
        { npm: false },
      ),
      timeoutMs: COMMAND_LIMITS.defaultTimeoutMs,
    });

  const npmRequest = (
    scriptName: string,
    profile: SandboxProfile,
    runPaths: CommandRunPaths,
  ): Promise<SandboxedProcessRequest> => {
    if (npmCli.status !== "resolved") {
      return Promise.reject(new Error(`npm CLI unavailable: ${npmCli.message}`));
    }
    return Promise.resolve({
      executable: process.execPath,
      arguments: [npmCli.cliPath, "run", scriptName, "--"],
      workingDirectory: join(options.workspaceRoot, NPM_PROBE_DIRECTORY),
      profile,
      environment: buildCommandEnvironment(
        {},
        {
          home: runPaths.home,
          temp: runPaths.temp,
          npmCache: runPaths.npmCache,
          npmUserConfig: runPaths.npmUserConfig,
        },
        { npm: true },
      ),
      timeoutMs: COMMAND_LIMITS.defaultTimeoutMs,
    });
  };

  const COMMAND_PROFILE = VALIDATION_OFFLINE_PROFILE;
  const probes: readonly Probe[] = [
    {
      id: "read-inside",
      description: "Read a file inside the workspace",
      buildRequest: (_context, runPaths) => nodeRequest("read-inside", options.profile, runPaths),
      check: (result) => result.stdout.includes("read-ok"),
    },
    {
      id: "write-inside",
      description: "Write a file inside the workspace (session profile)",
      buildRequest: (_context, runPaths) => nodeRequest("write-inside", options.profile, runPaths),
      check: (result) => result.stdout.includes("write-ok"),
      cleanupAfter: async (context) => {
        await rm(join(context.workspaceRoot, "probe-write.txt"), { force: true });
      },
    },
    {
      id: "write-outside",
      description: "Writes outside the workspace are denied",
      buildRequest: (_context, runPaths) => nodeRequest("write-outside", options.profile, runPaths),
      check: (result) => result.stdout.includes("escape-denied"),
    },
    {
      id: "read-secret",
      description: "Reading the denied fixture secret is denied",
      buildRequest: (_context, runPaths) => nodeRequest("read-secret", options.profile, runPaths),
      check: (result) => result.stdout.includes("secret-denied"),
    },
    {
      id: "network",
      description: "Outbound loopback connection is denied",
      needsLoopbackServer: true,
      buildRequest: (_context, runPaths) => nodeRequest("network", options.profile, runPaths),
      check: (result) => result.stdout.includes("network-denied"),
    },
    {
      id: "dns",
      description: "Outbound DNS resolution is denied",
      buildRequest: (_context, runPaths) => nodeRequest("dns", options.profile, runPaths),
      check: (result) => result.stdout.includes("dns-denied"),
    },
    {
      id: "env",
      description: "Provider secrets are absent from the child environment",
      buildRequest: (_context, runPaths) => nodeRequest("env", options.profile, runPaths),
      check: (result) => result.stdout.includes("secret-present:false"),
    },
    {
      id: "spawn",
      description: "A spawned descendant remains confined",
      buildRequest: (_context, runPaths) => nodeRequest("spawn", options.profile, runPaths),
      check: (result) => result.stdout.includes("descendant-confined"),
    },
    {
      id: "big-output",
      description: "Process output is bounded",
      profileOverrides: { maxOutputBytes: 1000 },
      buildRequest: (_context, runPaths) => nodeRequest("big-output", options.profile, runPaths),
      check: (result) => result.stdoutTruncated,
    },
    {
      id: "sleep",
      description: "Execution stops at the configured timeout",
      profileOverrides: { timeoutMs: 2000 },
      buildRequest: (_context, runPaths) => nodeRequest("sleep", options.profile, runPaths),
      check: (result) => result.status === "timed-out",
    },
    {
      id: "cancel",
      description: "Execution responds to cancellation",
      profileOverrides: { timeoutMs: 60_000 },
      cancelAfterMs: 300,
      buildRequest: (_context, runPaths) => nodeRequest("cancel", options.profile, runPaths),
      check: (result) => result.status === "cancelled",
    },
    {
      id: "node-read",
      description: "Node script reads a workspace fixture under validation-offline",
      profile: COMMAND_PROFILE,
      buildRequest: (_context, runPaths) => nodeRequest("node-read", COMMAND_PROFILE, runPaths),
      check: (result) => result.stdout.includes("read-ok"),
    },
    {
      id: "node-write-denied",
      description: "Node script cannot write the workspace",
      profile: COMMAND_PROFILE,
      buildRequest: (_context, runPaths) => nodeRequest("node-write", COMMAND_PROFILE, runPaths),
      check: (result) => result.stdout.includes("write-denied"),
    },
    {
      id: "node-child-write-denied",
      description: "A Node child cannot write the workspace",
      profile: COMMAND_PROFILE,
      buildRequest: (_context, runPaths) => nodeRequest("child-write", COMMAND_PROFILE, runPaths),
      check: (result) => result.stdout.includes("child-confined"),
    },
    {
      id: "node-grandchild-write-denied",
      description: "A Node grandchild cannot write the workspace",
      profile: COMMAND_PROFILE,
      buildRequest: (_context, runPaths) =>
        nodeRequest("grandchild-write", COMMAND_PROFILE, runPaths),
      check: (result) => result.stdout.includes("grandchild-confined"),
    },
    {
      id: "node-network-denied",
      description: "Node script cannot access outbound network",
      profile: COMMAND_PROFILE,
      buildRequest: (_context, runPaths) => nodeRequest("node-network", COMMAND_PROFILE, runPaths),
      check: (result) => result.stdout.includes("network-denied"),
    },
    {
      id: "node-loopback-denied",
      description: "Node script cannot access loopback",
      needsLoopbackServer: true,
      profile: COMMAND_PROFILE,
      buildRequest: (_context, runPaths) => nodeRequest("node-loopback", COMMAND_PROFILE, runPaths),
      check: (result) => result.stdout.includes("network-denied"),
    },
    {
      id: "node-env",
      description: "Provider credentials and NODE_OPTIONS are absent",
      profile: COMMAND_PROFILE,
      buildRequest: (_context, runPaths) => nodeRequest("node-env", COMMAND_PROFILE, runPaths),
      check: (result) =>
        result.stdout.includes("secret:absent") && result.stdout.includes("node-options:absent"),
    },
    {
      id: "npm-read",
      description: "npm script reads package files",
      profile: COMMAND_PROFILE,
      buildRequest: (_context, runPaths) => npmRequest("solaris-read", COMMAND_PROFILE, runPaths),
      check: (result) => result.stdout.includes("read-ok"),
    },
    {
      id: "npm-write-denied",
      description: "npm script cannot write the workspace",
      profile: COMMAND_PROFILE,
      buildRequest: (_context, runPaths) => npmRequest("solaris-write", COMMAND_PROFILE, runPaths),
      check: (result) => result.stdout.includes("write-denied"),
    },
    {
      id: "npm-network-denied",
      description: "npm script cannot access network",
      profile: COMMAND_PROFILE,
      buildRequest: (_context, runPaths) =>
        npmRequest("solaris-network", COMMAND_PROFILE, runPaths),
      check: (result) => result.stdout.includes("network-denied"),
    },
    {
      id: "npm-hooks",
      description: "npm pre/post scripts are not executed",
      profile: COMMAND_PROFILE,
      buildRequest: (_context, runPaths) => npmRequest("solaris-hooks", COMMAND_PROFILE, runPaths),
      check: (result) =>
        result.stdout.includes("target-ran") &&
        !result.stdout.includes("prehook-ran") &&
        !result.stdout.includes("posthook-ran"),
    },
    {
      id: "npm-stdin",
      description: "npm script receives no stdin",
      profile: COMMAND_PROFILE,
      buildRequest: (_context, runPaths) => npmRequest("solaris-stdin", COMMAND_PROFILE, runPaths),
      check: (result) => result.stdout.includes("stdin-closed"),
    },
    {
      id: "output-limit",
      description: "Output limit terminates the process",
      profile: COMMAND_PROFILE,
      profileOverrides: { maxOutputBytes: 2000 },
      timeoutMs: 30_000,
      buildRequest: (_context, runPaths) => nodeRequest("big-output", COMMAND_PROFILE, runPaths),
      check: (result) => result.status === "output-limit" && result.stdoutTruncated,
    },
    {
      id: "timeout-descendants",
      description: "Timeout terminates descendants that ignore normal termination",
      profile: COMMAND_PROFILE,
      timeoutMs: 2000,
      buildRequest: (_context, runPaths) => nodeRequest("heartbeat", COMMAND_PROFILE, runPaths),
      check: (result, context) =>
        result.status === "timed-out" && heartbeatStopped(context.runTemp),
    },
    {
      id: "cancel-descendants",
      description: "Cancellation terminates descendants that ignore normal termination",
      profile: COMMAND_PROFILE,
      profileOverrides: { timeoutMs: 60_000 },
      cancelAfterMs: 1500,
      buildRequest: (_context, runPaths) => nodeRequest("heartbeat", COMMAND_PROFILE, runPaths),
      check: (result, context) =>
        result.status === "cancelled" && heartbeatStopped(context.runTemp),
    },
    {
      id: "no-workspace-artifacts",
      description: "No unexpected sandbox files appear in the workspace",
      profile: COMMAND_PROFILE,
      buildRequest: (_context, runPaths) => nodeRequest("node-read", COMMAND_PROFILE, runPaths),
      check: async (_result, context) => {
        const expected = new Set([FIXTURE_FILE_NAME, PROBE_TARGET_FILE_NAME, NPM_PROBE_DIRECTORY]);
        const actual = await readdir(context.workspaceRoot);
        return actual.every((entry) => expected.has(entry));
      },
    },
    {
      id: "run-dir-cleanup",
      description: "Sandbox-private run directory cleanup succeeds",
      profile: COMMAND_PROFILE,
      buildRequest: async (_context, runPaths) => {
        const outcome = await runDirectories.remove(runPaths.runId);
        if (!outcome.ok) {
          throw new Error(outcome.message);
        }
        const entries = await readdir(runPaths.home).catch(() => []);
        if (entries.length > 0) {
          throw new Error("The run directory still exists after cleanup.");
        }
        return nodeRequest("node-read", COMMAND_PROFILE, runPaths);
      },
      check: (result) => result.stdout.includes("read-ok"),
    },
  ];

  const results: ConformanceProbeResult[] = [];
  let loopbackServer: Server | undefined;
  for (const probe of probes) {
    let loopbackPort = 0;
    if (probe.needsLoopbackServer === true) {
      loopbackServer = await startLoopbackServer();
      const address = loopbackServer.address();
      loopbackPort = typeof address === "object" && address !== null ? address.port : 0;
    }
    probeContext.loopbackPort = loopbackPort;
    const baseProfile = probe.profile ?? options.profile;
    const profile: SandboxProfile =
      probe.profileOverrides === undefined
        ? baseProfile
        : {
            ...baseProfile,
            process: { ...baseProfile.process, ...probe.profileOverrides },
          };
    const controller = new AbortController();
    const cancelTimer =
      probe.cancelAfterMs === undefined
        ? undefined
        : setTimeout(() => {
            controller.abort();
          }, probe.cancelAfterMs);
    let outcome: ConformanceProbeResult["outcome"];
    let detail: string;
    const runPaths = await runDirectories.create();
    probeContext.runTemp = runPaths.temp;
    try {
      let request: SandboxedProcessRequest;
      try {
        request = await probe.buildRequest(probeContext, runPaths);
      } finally {
        if (probe.id === "run-dir-cleanup") {
          await runDirectories.remove(runPaths.runId).catch(() => {});
        }
      }
      request = {
        ...request,
        profile,
        signal: controller.signal,
        ...(probe.timeoutMs === undefined ? {} : { timeoutMs: probe.timeoutMs }),
      };
      const result = await backend.execute(request);
      if (await probe.check(result, probeContext)) {
        outcome = "passed";
        detail = describeResult(result);
      } else {
        outcome = "failed";
        detail = describeResult(result);
      }
      if (probe.cleanupAfter !== undefined) {
        await probe.cleanupAfter(probeContext);
      }
    } catch (error: unknown) {
      outcome = "failed";
      detail = describeError(error);
    } finally {
      if (cancelTimer !== undefined) {
        clearTimeout(cancelTimer);
      }
      if (loopbackServer !== undefined) {
        await closeLoopbackServer(loopbackServer);
        loopbackServer = undefined;
      }
      await runDirectories.remove(runPaths.runId).catch(() => {});
    }
    results.push({ probeId: probe.id, description: probe.description, outcome, detail });
  }
  const passed = results.filter((result) => result.outcome === "passed").length;
  const failed = results.filter((result) => result.outcome === "failed").length;
  const skipped = results.filter((result) => result.outcome === "skipped").length;
  return {
    backendId: backend.id,
    platform: process.platform,
    profileId: options.profile.id,
    results,
    passed,
    failed,
    skipped,
  };
}

export async function removeConformanceArtifacts(workspaceRoot: string): Promise<void> {
  await rm(workspaceRoot, { recursive: true, force: true });
  await rm(join(dirname(workspaceRoot), `${basename(workspaceRoot)}-outside`), {
    recursive: true,
    force: true,
  });
  await rm(RUNS_ROOT, { recursive: true, force: true });
}

async function heartbeatStopped(runTemp: string): Promise<boolean> {
  const heartbeatPath = join(runTemp, HEARTBEAT_FILE_NAME);
  await new Promise<void>((resolve) => {
    setTimeout(resolve, HEARTBEAT_GRACE_MS);
  });
  const first = await readHeartbeat(heartbeatPath);
  await new Promise<void>((resolve) => {
    setTimeout(resolve, 500);
  });
  const second = await readHeartbeat(heartbeatPath);
  if (first === null || second === null) {
    return false;
  }
  return second === first;
}

async function readHeartbeat(path: string): Promise<string | null> {
  try {
    return await readFile(path, "utf8");
  } catch {
    return null;
  }
}

function describeResult(result: SandboxedProcessResult): string {
  const details: string[] = [
    `status=${result.status}`,
    `exit=${result.exitCode === null ? "none" : String(result.exitCode)}`,
  ];
  if (result.violations.length > 0) {
    details.push(
      `violations=${result.violations.map((violation) => violation.summary).join(" | ")}`,
    );
  }
  if (result.stdoutTruncated) {
    details.push("stdout truncated");
  }
  if (result.stdout.length > 0) {
    details.push(`stdout=${result.stdout.slice(0, 200)}`);
  }
  return details.join("; ");
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "Unknown conformance failure.";
}

async function startLoopbackServer(): Promise<Server> {
  const server = createServer();
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      resolve();
    });
  });
  return server;
}

async function closeLoopbackServer(server: Server): Promise<void> {
  await new Promise<void>((resolve) => {
    server.close(() => {
      resolve();
    });
  });
}

const NPM_PACKAGE_JSON = JSON.stringify({
  name: "solaris-conformance-package",
  private: true,
  scripts: {
    "solaris-read": "node solaris-npm-probe.cjs read",
    "solaris-write": "node solaris-npm-probe.cjs write",
    "solaris-network": "node solaris-npm-probe.cjs network",
    "pre-solaris-hooks": "node solaris-npm-probe.cjs prehook",
    "solaris-hooks": "node solaris-npm-probe.cjs target",
    "post-solaris-hooks": "node solaris-npm-probe.cjs posthook",
    "solaris-stdin": "node solaris-npm-probe.cjs stdin",
  },
});

const NPM_PROBE_SOURCE = `"use strict";
const fs = require("node:fs");
const path = require("node:path");
const net = require("node:net");

const mode = process.argv[2];

function report(marker) {
  process.stdout.write(marker + "\\n");
}

if (mode === "read") {
  try {
    fs.readFileSync(path.join(process.cwd(), "package.json"), "utf8");
    report("read-ok");
  } catch (error) {
    report("read-denied");
  }
} else if (mode === "write") {
  try {
    fs.writeFileSync(path.join(process.cwd(), "npm-probe-write.txt"), "x");
    report("write-ok");
  } catch (error) {
    report("write-denied");
  }
} else if (mode === "network") {
  const socket = net.connect({ host: "127.0.0.1", port: 80 });
  socket.setTimeout(5000);
  socket.once("connect", function () {
    socket.destroy();
    report("network-ok");
  });
  socket.once("error", function () {
    socket.destroy();
    report("network-denied");
  });
  socket.once("timeout", function () {
    socket.destroy();
    report("network-denied");
  });
} else if (mode === "target") {
  report("target-ran");
} else if (mode === "prehook") {
  report("prehook-ran");
} else if (mode === "posthook") {
  report("posthook-ran");
} else if (mode === "stdin") {
  try {
    const data = fs.readFileSync(0, { encoding: "utf8" });
    report(data.length === 0 ? "stdin-closed" : "stdin-open");
  } catch (error) {
    report("stdin-closed");
  }
}
`;

const FIXTURE_SOURCE = `"use strict";
const fs = require("node:fs");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");
const net = require("node:net");
const dns = require("node:dns");

const mode = process.argv[2];
const outsideDir = process.argv[3];
const secretPath = process.argv[4];
const port = Number(process.argv[5]);
const heartbeatPath = process.argv[6];

function report(marker) {
  process.stdout.write(marker);
}

if (mode === "read-inside" || mode === "node-read") {
  try {
    fs.readFileSync(path.join(process.cwd(), "probe-target.txt"), "utf8");
    report("read-ok");
  } catch (error) {
    report("read-denied");
  }
} else if (mode === "write-inside") {
  try {
    fs.writeFileSync(path.join(process.cwd(), "probe-write.txt"), "ok");
    report("write-ok");
  } catch (error) {
    report("write-denied");
  }
} else if (mode === "node-write") {
  try {
    fs.writeFileSync(path.join(process.cwd(), "probe-write.txt"), "x");
    report("write-ok");
  } catch (error) {
    report("write-denied");
  }
} else if (mode === "child-write") {
  const child = spawnSync(
    process.execPath,
    [process.argv[1], "node-write", outsideDir, secretPath],
    { encoding: "utf8", timeout: 10000 }
  );
  const output = child.stdout ? child.stdout : "";
  report(output.indexOf("write-denied") >= 0 ? "child-confined" : "child-escape");
} else if (mode === "grandchild-write") {
  const child = spawnSync(
    process.execPath,
    [process.argv[1], "child-write", outsideDir, secretPath],
    { encoding: "utf8", timeout: 10000 }
  );
  const output = child.stdout ? child.stdout : "";
  report(output.indexOf("child-confined") >= 0 ? "grandchild-confined" : "grandchild-escape");
} else if (mode === "write-outside") {
  try {
    fs.writeFileSync(path.join(outsideDir, "escape.txt"), "x");
    report("escape-ok");
  } catch (error) {
    report("escape-denied");
  }
} else if (mode === "read-secret") {
  try {
    fs.readFileSync(secretPath, "utf8");
    report("secret-ok");
  } catch (error) {
    report("secret-denied");
  }
} else if (mode === "network" || mode === "node-network") {
  const host = mode === "node-network" ? "192.0.2.1" : "127.0.0.1";
  const socket = net.connect({ host: host, port: mode === "node-network" ? 53 : port });
  socket.setTimeout(5000);
  socket.once("connect", function () {
    socket.destroy();
    report("network-ok");
  });
  socket.once("error", function () {
    socket.destroy();
    report("network-denied");
  });
  socket.once("timeout", function () {
    socket.destroy();
    report("network-denied");
  });
} else if (mode === "node-loopback") {
  const socket = net.connect({ host: "127.0.0.1", port: port });
  socket.setTimeout(5000);
  socket.once("connect", function () {
    socket.destroy();
    report("network-ok");
  });
  socket.once("error", function () {
    socket.destroy();
    report("network-denied");
  });
  socket.once("timeout", function () {
    socket.destroy();
    report("network-denied");
  });
} else if (mode === "dns") {
  dns.lookup("solaris-conformance.invalid", function (error) {
    report(error ? "dns-denied" : "dns-resolved");
  });
} else if (mode === "env") {
  report(process.env.OPENROUTER_API_KEY ? "secret-present:true" : "secret-present:false");
} else if (mode === "node-env") {
  report(
    "secret:" + (process.env.OPENROUTER_API_KEY ? "present" : "absent") +
    ";node-options:" + (process.env.NODE_OPTIONS ? "present" : "absent")
  );
} else if (mode === "spawn") {
  const child = spawnSync(
    process.execPath,
    [process.argv[1], "write-outside", outsideDir, secretPath],
    { encoding: "utf8", timeout: 10000 }
  );
  const output = child.stdout ? child.stdout : "";
  report(output.indexOf("escape-denied") >= 0 ? "descendant-confined" : "descendant-escape");
} else if (mode === "big-output") {
  report("x".repeat(200000));
} else if (mode === "sleep") {
  const end = Date.now() + 30000;
  while (Date.now() < end) {
    // busy wait so the sandbox must actively stop the process
  }
} else if (mode === "heartbeat") {
  const writer = spawn(
    process.execPath,
    [process.argv[1], "heartbeat-writer", heartbeatPath],
    { detached: true, stdio: "ignore" }
  );
  writer.unref();
  const end = Date.now() + 60000;
  while (Date.now() < end) {
    // busy wait; descendants keep writing the heartbeat
  }
} else if (mode === "heartbeat-writer") {
  const target = process.argv[3];
  const interval = setInterval(function () {
    try {
      fs.writeFileSync(target, String(Date.now()));
    } catch (error) {
      // heartbeat path may disappear during teardown
    }
  }, 100);
  process.on("SIGTERM", function () {
    clearInterval(interval);
    process.exit(0);
  });
  setInterval(function () {}, 1000);
}
`;
