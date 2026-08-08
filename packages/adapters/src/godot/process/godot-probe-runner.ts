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
import type { CommandRunPaths, RunDirectoryProvider } from "../../process/run-directories.js";
import { extractGodotApiDumpSummary } from "../api-dump/api-dump-summary.js";
import { stageVerifiedExecutableCopy } from "./executable-copy.js";
import { parseGodotVersionText } from "./version-parser.js";
import { parseHelpCapabilities } from "./help-capabilities-parser.js";

/**
 * Fixed Solaris probe invocations. The adapter chooses every argument and
 * no other argument array may ever be constructed for a Godot probe.
 * Project-affecting options are prohibited here by the architecture check.
 *
 * `fixedProbeArguments` is THE single constructor of Godot probe argument
 * tuples. It is private to this adapter module: no other module may build,
 * import, or re-export a Godot probe argument array, and the architecture
 * check verifies that only the three fixed tuples ever appear in probe
 * invocation code. Every sandboxed Godot probe request below derives its
 * arguments exclusively from this constructor.
 */
type GodotFixedProbeKind = "version" | "help" | "api-dump";

function fixedProbeArguments(kind: GodotFixedProbeKind): readonly string[] {
  switch (kind) {
    case "version":
      return ["--version"];
    case "help":
      return ["--help"];
    case "api-dump":
      return ["--dump-extension-api"];
  }
}

export interface GodotProbeRunnerDependencies {
  readonly backend: SandboxBackend;
  readonly runDirectories: RunDirectoryProvider;
  /** Sanitized host parent environment (never raw `process.env`). */
  readonly parentEnvironment: Readonly<Record<string, string>>;
}

/**
 * Godot probe execution is bound to the fingerprinted executable bytes:
 *
 * 1. Revalidate: the complete bounded SHA-256 of the canonical path is
 *    recomputed before every probe and must equal the validated identity.
 * 2. Stage: a private executable copy is created exclusively inside the
 *    probe's run directory, size-bounded, and chmod 0755 on POSIX.
 * 3. Verify: the copy's complete SHA-256 must equal the validated
 *    fingerprint before execution; a mismatch fails the probe closed.
 * 4. Execute: only the private copy path is executed — never the mutable
 *    configured path.
 *
 * Probes are project-independent: requests carry no workspace root in the
 * executable, arguments, working directory, environment, or sandbox
 * configuration (`explicitReadRoots` is empty; the run directory grants
 * everything the copy needs). Sandbox enforcement requires every
 * restriction (read, write, network, process tree) plus an available state.
 */
export function createGodotProbeRunner(
  dependencies: GodotProbeRunnerDependencies,
): GodotProbeRunner {
  async function probeVersion(
    installation: GodotInstallation,
    signal?: AbortSignal,
  ): Promise<GodotVersionProbe> {
    const outcome = await runProbe(installation, fixedProbeArguments("version"), {
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
    const outcome = await runProbe(installation, fixedProbeArguments("help"), {
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
    const prepared = await beginProbe(installation, signal);
    if (prepared === null) {
      return { status: "failed", message: "The probe sandbox is unavailable." };
    }
    const { runPaths, executablePath, release } = prepared;
    let result: SandboxedProcessResult;
    try {
      result = await dependencies.backend.execute(
        probeRequest(fixedProbeArguments("api-dump"), executablePath, runPaths, {
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
    if (result.exitCode !== 0) {
      await release();
      return {
        status: "degraded",
        message: `The API dump probe exited with code ${String(result.exitCode)}; its dump output was not trusted.`,
      };
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

  interface PreparedProbeRun {
    readonly runPaths: CommandRunPaths;
    /** Verified private copy path; the only path ever executed. */
    readonly executablePath: string;
    readonly release: () => Promise<string | null>;
  }

  /**
   * Common probe-run preparation: full identity revalidation, sandbox
   * enforcement, private run directory creation, and private executable
   * copy staging. Any failure fails the probe closed with a precise message
   * and never falls back to executing the mutable configured path.
   */
  async function prepareProbeRun(
    installation: GodotInstallation,
    signal: AbortSignal | undefined,
  ): Promise<
    | { readonly ok: true; readonly value: PreparedProbeRun }
    | { readonly ok: false; readonly error: string }
  > {
    if (signal?.aborted) {
      throw createAbortError();
    }
    const identity = await revalidateInstallation(installation);
    if (!identity.ok) {
      return { ok: false, error: identity.error };
    }
    const sandbox = await sandboxEnforced();
    if (!sandbox.enforced) {
      return { ok: false, error: sandbox.message };
    }
    let runPaths: CommandRunPaths;
    try {
      runPaths = await dependencies.runDirectories.create();
    } catch {
      return {
        ok: false,
        error: "The private probe directory could not be created; the probe did not run.",
      };
    }
    const release = async (): Promise<string | null> => {
      const cleanup = await dependencies.runDirectories.remove(runPaths.runId);
      return cleanup.ok ? null : cleanup.message;
    };
    const staged = await stageVerifiedExecutableCopy({
      sourcePath: installation.canonicalPath,
      runRoot: runPaths.root,
      expectedSha256: installation.sha256,
      maxBytes: GODOT_LIMITS.maxExecutableBytes,
      ...(signal === undefined ? {} : { signal }),
    });
    if (!staged.ok) {
      await release();
      if (signal?.aborted) {
        throw createAbortError();
      }
      return { ok: false, error: staged.error };
    }
    return { ok: true, value: { runPaths, executablePath: staged.copyPath, release } };
  }

  async function beginProbe(
    installation: GodotInstallation,
    signal?: AbortSignal,
  ): Promise<PreparedProbeRun | null> {
    const prepared = await prepareProbeRun(installation, signal);
    return prepared.ok ? prepared.value : null;
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
    const prepared = await prepareProbeRun(installation, limits.signal);
    if (!prepared.ok) {
      return { status: "failed", message: prepared.error, result: emptyResult() };
    }
    const { runPaths, executablePath, release } = prepared.value;
    let result: SandboxedProcessResult;
    try {
      result = await dependencies.backend.execute(
        probeRequest(arguments_, executablePath, runPaths, limits),
      );
    } catch (error: unknown) {
      await release();
      if (limits.signal?.aborted) {
        throw createAbortError();
      }
      return { status: "failed", message: describeProbeFailure(error), result: emptyResult() };
    }
    const cleanupMessage = await release();
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
    if (cleanupMessage !== null) {
      return {
        status: "failed",
        message: `The probe completed, but its private directory could not be cleaned: ${cleanupMessage}`,
        result,
      };
    }
    return { status: "completed", message: "", result };
  }

  function probeRequest(
    arguments_: readonly string[],
    executablePath: string,
    runPaths: {
      readonly root: string;
      readonly home: string;
      readonly temp: string;
    },
    limits: {
      readonly timeoutMs: number;
      readonly stdoutLimitBytes: number;
      readonly stderrLimitBytes: number;
      readonly signal: AbortSignal | undefined;
    },
  ): SandboxedProcessRequest {
    return {
      executable: executablePath,
      arguments: arguments_,
      workingDirectory: runPaths.temp,
      profile: GODOT_PROBE_OFFLINE_PROFILE,
      environment: buildChildEnvironment(dependencies.parentEnvironment, {
        home: runPaths.home,
        temp: runPaths.temp,
      }),
      runDirectory: runPaths.root,
      // Exact read-only mode: the private copy lives inside the run
      // directory; no additional host roots are required, so the broad
      // trusted-runner/workspace surfaces are not granted by the backend.
      explicitReadRoots: [],
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

  async function sandboxEnforced(): Promise<
    { readonly enforced: true } | { readonly enforced: false; readonly message: string }
  > {
    let status: SandboxBackendStatus;
    try {
      status = await dependencies.backend.inspect();
    } catch {
      return {
        enforced: false,
        message:
          "The sandbox state could not be inspected; the Godot probe did not run (fail closed).",
      };
    }
    if (status.state !== "available") {
      if (process.platform === "win32") {
        return {
          enforced: false,
          message:
            "The sandbox is unavailable; host-read enforcement is not available on this platform; the Godot probe did not run (fail closed).",
        };
      }
      return {
        enforced: false,
        message: `The sandbox is unavailable (state: ${status.state}); the Godot probe did not run (fail closed).`,
      };
    }
    const missing: string[] = [];
    if (!status.capabilities.filesystemReadRestriction) {
      missing.push("filesystem read restriction");
    }
    if (!status.capabilities.filesystemWriteRestriction) {
      missing.push("filesystem write restriction");
    }
    if (!status.capabilities.networkRestriction) {
      missing.push("network restriction");
    }
    if (!status.capabilities.processTreeRestriction) {
      missing.push("process-tree restriction");
    }
    if (missing.length > 0) {
      return {
        enforced: false,
        message: `The sandbox lacks ${missing.join(", ")}; the Godot probe did not run (fail closed).`,
      };
    }
    return { enforced: true };
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
  // Only the fixed `extension_api.json` inside the private probe temp is
  // examined; any other output files are ignored and removed with the run.
  const dumpPath = `${directory}${process.platform === "win32" ? "\\" : "/"}extension_api.json`;
  let metadata;
  try {
    metadata = await lstat(dumpPath);
  } catch {
    return { ok: false, message: "The API dump file could not be inspected." };
  }
  if (metadata.isSymbolicLink() || !metadata.isFile()) {
    return { ok: false, message: "The API dump output is not a regular file; rejecting it." };
  }
  if (metadata.size > GODOT_LIMITS.maxApiDumpBytes) {
    return {
      ok: false,
      message: `The API dump exceeds the ${Math.round(GODOT_LIMITS.maxApiDumpBytes / (1024 * 1024))} MiB limit; the dump failed safely.`,
    };
  }
  let content: Buffer;
  try {
    const handle = await open(dumpPath, "r");
    try {
      const buffer = Buffer.alloc(metadata.size);
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
      if (bytesRead !== metadata.size) {
        return {
          ok: false,
          message: "The API dump file changed while it was read; rejecting it.",
        };
      }
      content = buffer.subarray(0, bytesRead);
    } finally {
      await handle.close().catch(() => undefined);
    }
  } catch {
    return { ok: false, message: "The API dump could not be read." };
  }
  const summary = extractGodotApiDumpSummary(content);
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
