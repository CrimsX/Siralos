import { mkdir, rm, writeFile } from "node:fs/promises";
import { createServer, type Server } from "node:net";
import { basename, dirname, join } from "node:path";
import type {
  SandboxBackend,
  SandboxProfile,
  SandboxedProcessRequest,
  SandboxedProcessResult,
} from "@solaris/core";
import { buildChildEnvironment } from "../../environment/child-environment.js";
import { getSandboxDirectories } from "../sandbox-directories.js";

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

interface Probe {
  readonly id: string;
  readonly description: string;
  readonly profileOverrides?: Partial<SandboxProfile["process"]>;
  readonly needsLoopbackServer?: boolean;
  readonly cancelAfterMs?: number;
  readonly check: (result: SandboxedProcessResult) => boolean;
}

export async function runSandboxConformance(
  backend: SandboxBackend,
  options: ConformanceOptions,
): Promise<ConformanceReport> {
  const sandboxDirectories = getSandboxDirectories();
  const fixturePath = join(options.workspaceRoot, FIXTURE_FILE_NAME);
  await writeFile(fixturePath, FIXTURE_SOURCE);
  await writeFile(join(options.workspaceRoot, PROBE_TARGET_FILE_NAME), "probe-target\n");
  const outsideDirectory = join(
    dirname(options.workspaceRoot),
    `${basename(options.workspaceRoot)}-outside`,
  );
  await mkdir(outsideDirectory, { recursive: true });
  const secretPath = join(outsideDirectory, OUTSIDE_SECRET_FILE_NAME);
  await writeFile(secretPath, "PROBE-SECRET-DO-NOT-USE\n");

  const environment = buildChildEnvironment(options.parentEnvironment, sandboxDirectories);
  const results: ConformanceProbeResult[] = [];
  let loopbackServer: Server | undefined;
  for (const probe of PROBES) {
    let loopbackPort = 0;
    if (probe.needsLoopbackServer === true) {
      loopbackServer = await startLoopbackServer();
      const address = loopbackServer.address();
      loopbackPort = typeof address === "object" && address !== null ? address.port : 0;
    }
    const profile: SandboxProfile =
      probe.profileOverrides === undefined
        ? options.profile
        : {
            ...options.profile,
            process: { ...options.profile.process, ...probe.profileOverrides },
          };
    const controller = new AbortController();
    const cancelTimer =
      probe.cancelAfterMs === undefined
        ? undefined
        : setTimeout(() => {
            controller.abort();
          }, probe.cancelAfterMs);
    const request: SandboxedProcessRequest = {
      executable: process.execPath,
      arguments: [fixturePath, probe.id, outsideDirectory, secretPath, String(loopbackPort)],
      workingDirectory: options.workspaceRoot,
      profile,
      environment,
      signal: controller.signal,
    };
    let outcome: ConformanceProbeResult["outcome"];
    let detail: string;
    try {
      const result = await backend.execute(request);
      if (probe.check(result)) {
        outcome = "passed";
        detail = describeResult(result);
      } else {
        outcome = "failed";
        detail = describeResult(result);
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
}

const PROBES: readonly Probe[] = [
  {
    id: "read-inside",
    description: "Read a file inside the workspace",
    check: (result) => result.stdout.includes("read-ok"),
  },
  {
    id: "write-inside",
    description: "Write a file inside the workspace",
    check: (result) => result.stdout.includes("write-ok"),
  },
  {
    id: "write-outside",
    description: "Writes outside the workspace are denied",
    check: (result) => result.stdout.includes("escape-denied"),
  },
  {
    id: "read-secret",
    description: "Reading the denied fixture secret is denied",
    check: (result) => result.stdout.includes("secret-denied"),
  },
  {
    id: "network",
    description: "Outbound loopback connection is denied",
    needsLoopbackServer: true,
    check: (result) => result.stdout.includes("network-denied"),
  },
  {
    id: "dns",
    description: "Outbound DNS resolution is denied",
    check: (result) => result.stdout.includes("dns-denied"),
  },
  {
    id: "env",
    description: "Provider secrets are absent from the child environment",
    check: (result) => result.stdout.includes("secret-present:false"),
  },
  {
    id: "spawn",
    description: "A spawned descendant remains confined",
    check: (result) => result.stdout.includes("descendant-confined"),
  },
  {
    id: "big-output",
    description: "Process output is bounded",
    profileOverrides: { maxOutputBytes: 1000 },
    check: (result) => result.stdoutTruncated,
  },
  {
    id: "sleep",
    description: "Execution stops at the configured timeout",
    profileOverrides: { timeoutMs: 2000 },
    check: (result) => result.status === "timed-out",
  },
  {
    id: "cancel",
    description: "Execution responds to cancellation",
    profileOverrides: { timeoutMs: 60_000 },
    cancelAfterMs: 300,
    check: (result) => result.status === "cancelled",
  },
];

function describeResult(result: SandboxedProcessResult): string {
  const details: string[] = [`status=${result.status}`, `exit=${result.exitCode ?? "none"}`];
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

const FIXTURE_SOURCE = `"use strict";
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const net = require("node:net");
const dns = require("node:dns");

const mode = process.argv[2];
const outsideDir = process.argv[3];
const secretPath = process.argv[4];
const port = Number(process.argv[5]);

function report(marker) {
  process.stdout.write(marker);
}

if (mode === "read-inside") {
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
} else if (mode === "network") {
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
}
`;
