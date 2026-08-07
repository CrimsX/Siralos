import {
  GODOT_LIMITS,
  GODOT_PROBE_OFFLINE_PROFILE,
  type GodotApiDumpProbe,
  type GodotHelpProbe,
  type GodotInstallation,
  type GodotProbeRunner,
  type GodotVersionProbe,
  type SandboxBackend,
  type SandboxBackendStatus,
  type SandboxedProcessRequest,
  type SandboxedProcessResult,
} from "@solaris/core";
import { buildChildEnvironment } from "../../environment/child-environment.js";
import type { RunDirectoryProvider } from "../../process/run-directories.js";
import { extractGodotApiDumpSummary } from "../api-dump/api-dump-summary.js";
import { parseGodotVersionText } from "./version-parser.js";
import { parseHelpCapabilities } from "./help-capabilities-parser.js";

/**
 * Fixed Solaris probe invocations. The adapter chooses every argument and
 * no other argument array may ever be constructed for a Godot probe.
 * Project-affecting options are prohibited here by the architecture check.
 */
export const GODOT_VERSION_ARGUMENTS: readonly string[] = ["--version"];
export const GODOT_HELP_ARGUMENTS: readonly string[] = ["--help"];
export const GODOT_API_DUMP_ARGUMENTS: readonly string[] = ["--dump-extension-api"];

export interface GodotProbeRunnerDependencies {
  readonly backend: SandboxBackend;
  readonly runDirectories: RunDirectoryProvider;
  /** Sanitized host parent environment (never raw `process.env`). */
  readonly parentEnvironment: Readonly<Record<string, string>>;
}

export function createGodotProbeRunner(
  dependencies: GodotProbeRunnerDependencies,
): GodotProbeRunner {
  async function probeVersion(
    installation: GodotInstallation,
    signal?: AbortSignal,
  ): Promise<GodotVersionProbe> {
    const outcome = await runProbe(installation, GODOT_VERSION_ARGUMENTS, {
      timeoutMs: GODOT_LIMITS.versionProbeTimeoutMs,
      stdoutLimitBytes: GODOT_LIMITS.maxVersionOutputBytes,
      stderrLimitBytes: GODOT_LIMITS.maxVersionOutputBytes,
      signal,
    });
    if (outcome.status !== "completed") {
      return { status: "failed", message: outcome.message };
    }
    const text = pickProbeOutput(outcome.result);
    if (text.length === 0) {
      return { status: "failed", message: "The Godot version output is empty." };
    }
    const parsed = parseGodotVersionText(text);
    if (!parsed.ok) {
      return { status: "failed", message: parsed.message };
    }
    return { status: "success", version: parsed.version };
  }

  async function probeHelp(
    installation: GodotInstallation,
    signal?: AbortSignal,
  ): Promise<GodotHelpProbe> {
    const outcome = await runProbe(installation, GODOT_HELP_ARGUMENTS, {
      timeoutMs: GODOT_LIMITS.helpProbeTimeoutMs,
      stdoutLimitBytes: GODOT_LIMITS.maxHelpOutputBytes,
      stderrLimitBytes: GODOT_LIMITS.maxHelpOutputBytes,
      signal,
    });
    const text = pickProbeOutput(outcome.result);
    if (text.length === 0) {
      return { status: "failed", message: outcome.message };
    }
    const parsed = parseHelpCapabilities(text);
    if (outcome.status === "completed") {
      return { status: "success", ...parsed };
    }
    return {
      status: "degraded",
      message: outcome.message,
      capabilities: parsed.capabilities,
      unknownOptionCount: parsed.unknownOptionCount,
    };
  }

  async function dumpExtensionApi(
    installation: GodotInstallation,
    signal?: AbortSignal,
  ): Promise<GodotApiDumpProbe> {
    const probeDir = await beginProbe(installation, signal);
    if (probeDir === null) {
      return { status: "failed", message: "The probe sandbox is unavailable." };
    }
    const { runPaths, release } = probeDir;
    let result: SandboxedProcessResult;
    try {
      result = await dependencies.backend.execute(
        probeRequest(installation, GODOT_API_DUMP_ARGUMENTS, runPaths, {
          timeoutMs: GODOT_LIMITS.apiDumpTimeoutMs,
          stdoutLimitBytes: 1024 * 1024,
          stderrLimitBytes: 1024 * 1024,
          signal,
        }),
      );
    } catch (error: unknown) {
      await release();
      return { status: "failed", message: describeProbeFailure(error) };
    }
    if (result.status === "cancelled" || signal?.aborted) {
      await release();
      throw createAbortError();
    }
    if (result.status !== "completed") {
      await release();
      return { status: "failed", message: describeProbeOutcome(result) };
    }
    const summary = await extractApiDumpFromDirectory(runPaths.temp);
    const cleanupMessage = await release();
    if (!summary.ok) {
      return { status: "degraded", message: summary.message };
    }
    if (cleanupMessage !== null) {
      return {
        status: "degraded",
        message: `The API dump was parsed, but the probe directory could not be cleaned: ${cleanupMessage}`,
      };
    }
    return { status: "success", summary: summary.summary };
  }

  async function beginProbe(
    installation: GodotInstallation,
    signal?: AbortSignal,
  ): Promise<{
    readonly runPaths: { readonly home: string; readonly temp: string };
    readonly release: () => Promise<string | null>;
  } | null> {
    if (signal?.aborted) {
      throw createAbortError();
    }
    const identity = await revalidateInstallation(installation);
    if (!identity.ok) {
      return null;
    }
    if (!(await sandboxEnforced())) {
      return null;
    }
    let runPaths;
    try {
      runPaths = await dependencies.runDirectories.create();
    } catch {
      return null;
    }
    return {
      runPaths,
      release: async () => {
        const cleanup = await dependencies.runDirectories.remove(runPaths.runId);
        return cleanup.ok ? null : cleanup.message;
      },
    };
  }

  async function runProbe(
    installation: GodotInstallation,
    arguments_: readonly string[],
    limits: {
      readonly timeoutMs: number;
      readonly stdoutLimitBytes: number;
      readonly stderrLimitBytes: number;
      readonly signal: AbortSignal | undefined;
    },
  ): Promise<{ status: "completed" | "failed"; message: string; result: SandboxedProcessResult }> {
    if (limits.signal?.aborted) {
      throw createAbortError();
    }
    const identity = await revalidateInstallation(installation);
    if (!identity.ok) {
      return { status: "failed", message: identity.error, result: emptyResult() };
    }
    if (!(await sandboxEnforced())) {
      return {
        status: "failed",
        message: "The sandbox is unavailable; the Godot probe did not run (fail closed).",
        result: emptyResult(),
      };
    }
    let runPaths;
    try {
      runPaths = await dependencies.runDirectories.create();
    } catch {
      return {
        status: "failed",
        message: "The private probe directory could not be created; the probe did not run.",
        result: emptyResult(),
      };
    }
    let result: SandboxedProcessResult;
    try {
      result = await dependencies.backend.execute(
        probeRequest(installation, arguments_, runPaths, limits),
      );
    } catch (error: unknown) {
      await dependencies.runDirectories.remove(runPaths.runId).catch(() => undefined);
      if (limits.signal?.aborted) {
        throw createAbortError();
      }
      return { status: "failed", message: describeProbeFailure(error), result: emptyResult() };
    }
    const cleanup = await dependencies.runDirectories.remove(runPaths.runId);
    if (result.status === "cancelled" || limits.signal?.aborted) {
      throw createAbortError();
    }
    if (result.status !== "completed") {
      return { status: "failed", message: describeProbeOutcome(result), result };
    }
    if (result.exitCode !== 0) {
      return {
        status: "failed",
        message: `The Godot probe exited with code ${String(result.exitCode)}.`,
        result,
      };
    }
    if (!cleanup.ok) {
      return {
        status: "failed",
        message: `The probe completed, but its private directory could not be cleaned: ${cleanup.message}`,
        result,
      };
    }
    return { status: "completed", message: "", result };
  }

  function probeRequest(
    installation: GodotInstallation,
    arguments_: readonly string[],
    runPaths: { readonly home: string; readonly temp: string },
    limits: {
      readonly timeoutMs: number;
      readonly stdoutLimitBytes: number;
      readonly stderrLimitBytes: number;
      readonly signal: AbortSignal | undefined;
    },
  ): SandboxedProcessRequest {
    return {
      executable: installation.canonicalPath,
      arguments: arguments_,
      workingDirectory: runPaths.temp,
      profile: GODOT_PROBE_OFFLINE_PROFILE,
      environment: buildChildEnvironment(dependencies.parentEnvironment, {
        home: runPaths.home,
        temp: runPaths.temp,
      }),
      timeoutMs: limits.timeoutMs,
      stdoutLimitBytes: limits.stdoutLimitBytes,
      stderrLimitBytes: limits.stderrLimitBytes,
      ...(limits.signal === undefined ? {} : { signal: limits.signal }),
    };
  }

  async function revalidateInstallation(
    installation: GodotInstallation,
  ): Promise<{ readonly ok: true } | { readonly ok: false; readonly error: string }> {
    if (installation.status !== "valid") {
      return { ok: false, error: "The installation is invalid; rediscovery is required." };
    }
    const { revalidateExecutableIdentity } = await import("../discovery/executable-validation.js");
    const result = await revalidateExecutableIdentity({
      canonicalPath: installation.canonicalPath,
      sizeBytes: installation.sizeBytes,
      modifiedAtMs: installation.modifiedAtMs,
      sha256: installation.sha256,
    });
    return result.unchanged ? { ok: true } : { ok: false, error: result.error };
  }

  async function sandboxEnforced(): Promise<boolean> {
    let status: SandboxBackendStatus;
    try {
      status = await dependencies.backend.inspect();
    } catch {
      return false;
    }
    return (
      status.state === "available" &&
      status.capabilities.filesystemWriteRestriction &&
      status.capabilities.networkRestriction &&
      status.capabilities.processTreeRestriction
    );
  }

  return { probeVersion, probeHelp, dumpExtensionApi };
}

async function extractApiDumpFromDirectory(
  directory: string,
): Promise<
  | { readonly ok: true; readonly summary: import("@solaris/core").GodotApiDumpSummary }
  | { readonly ok: false; readonly message: string }
> {
  const { readdir, lstat, open } = await import("node:fs/promises");
  let entries: string[];
  try {
    entries = await readdir(directory);
  } catch {
    return { ok: false, message: "The probe directory could not be listed." };
  }
  if (!entries.includes("extension_api.json")) {
    return {
      ok: false,
      message:
        "The API dump command completed, but no extension_api.json was produced; the capability is advertised but operationally failed.",
    };
  }
  const dumpPath = `${directory}${process.platform === "win32" ? "\\" : "/"}extension_api.json`;
  let metadata;
  try {
    metadata = await lstat(dumpPath);
  } catch {
    return { ok: false, message: "The API dump file could not be inspected." };
  }
  if (!metadata.isFile()) {
    return { ok: false, message: "The API dump output is not a regular file; rejecting it." };
  }
  if (metadata.size > GODOT_LIMITS.maxApiDumpBytes) {
    return {
      ok: false,
      message: `The API dump exceeds the ${Math.round(GODOT_LIMITS.maxApiDumpBytes / (1024 * 1024))} MiB limit; the dump failed safely.`,
    };
  }
  let content: string;
  try {
    const handle = await open(dumpPath, "r");
    try {
      const buffer = Buffer.alloc(metadata.size);
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
      content = buffer.subarray(0, bytesRead).toString("utf8");
    } finally {
      await handle.close().catch(() => undefined);
    }
  } catch {
    return { ok: false, message: "The API dump could not be read." };
  }
  const summary = extractGodotApiDumpSummary(content, metadata.size);
  if (!summary.ok) {
    return { ok: false, message: summary.message };
  }
  return { ok: true, summary: summary.summary };
}

function pickProbeOutput(result: SandboxedProcessResult): string {
  const stdout = result.stdout.trim();
  if (stdout.length > 0) {
    return stdout;
  }
  const stderr = result.stderr.trim();
  if (stderr.length > 0) {
    return stderr;
  }
  return "";
}

function describeProbeOutcome(result: SandboxedProcessResult): string {
  switch (result.status) {
    case "timed-out":
      return "The Godot probe timed out and its process tree was terminated.";
    case "output-limit":
      return "The Godot probe exceeded its output limit and was terminated.";
    case "sandbox-denied":
      return `The sandbox denied part of the probe: ${result.violations.map((violation) => violation.summary).join("; ")}`;
    case "sandbox-unavailable":
      return "The sandbox became unavailable during the probe; the probe failed closed.";
    case "cancelled":
      return "The Godot probe was cancelled.";
    case "failed":
      return "The Godot probe process failed to run.";
    case "completed":
      return "The Godot probe completed.";
  }
}

function describeProbeFailure(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unknown Godot probe failure occurred.";
}

function emptyResult(): SandboxedProcessResult {
  return {
    status: "failed",
    exitCode: null,
    signal: null,
    stdout: "",
    stderr: "",
    stdoutTruncated: false,
    stderrTruncated: false,
    durationMs: 0,
    violations: [],
  };
}

function createAbortError(): Error {
  return new DOMException("The Godot probe was aborted.", "AbortError");
}
