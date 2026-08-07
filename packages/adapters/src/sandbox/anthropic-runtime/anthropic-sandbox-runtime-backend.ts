import type {
  SandboxBackend,
  SandboxBackendStatus,
  SandboxProfile,
  SandboxedProcessRequest,
  SandboxedProcessResult,
  SandboxViolation,
} from "@solaris/core";
import { COMMAND_LIMITS, SandboxError } from "@solaris/core";
import {
  SandboxManager,
  VENDORED_SRT_WIN_EXE,
  checkWindowsSandboxStatusAsync,
  resolveSrtWin,
  windowsInstallInstructions,
  type SandboxRuntimeConfig,
} from "@anthropic-ai/sandbox-runtime";
import { spawn, type ChildProcess } from "node:child_process";
import { StringDecoder } from "node:string_decoder";
import { homedir } from "node:os";
import path from "node:path";

export const ANTHROPIC_SANDBOX_RUNTIME_BACKEND_ID = "anthropic-runtime";

export const ANTHROPIC_SANDBOX_RUNTIME_VERSION = "0.0.70";

export interface AnthropicSandboxRuntimeBackendOptions {
  readonly workspaceRoot: string;
  readonly sandboxHome: string;
  readonly sandboxTemp: string;
  /**
   * Solaris-owned runs root (`~/.solaris/runs`). Command run directories live
   * beneath it; it is readable and writable only for sandboxed commands.
   */
  readonly runRoot?: string;
}

export function createAnthropicSandboxRuntimeBackend(
  options: AnthropicSandboxRuntimeBackendOptions,
): SandboxBackend {
  let initializedProfile: SandboxProfile | undefined;
  let closed = false;

  async function inspect(): Promise<SandboxBackendStatus> {
    const platform = detectPlatform();
    if (platform === "unknown" || !SandboxManager.isSupportedPlatform()) {
      return unsupportedStatus(platform);
    }
    if (platform === "windows") {
      return inspectWindows();
    }
    if (platform === "linux") {
      const dependencies = await SandboxManager.checkDependenciesAsync();
      if (dependencies.errors.length > 0) {
        return {
          ...baseStatus("dependency-missing", platform),
          message: `Missing sandbox dependencies: ${dependencies.errors.join("; ")}`,
        };
      }
      return baseStatus("available", platform);
    }
    return baseStatus("available", platform);
  }

  async function inspectWindows(): Promise<SandboxBackendStatus> {
    const srtWin = resolveSrtWin({ path: VENDORED_SRT_WIN_EXE });
    let status;
    try {
      status = await checkWindowsSandboxStatusAsync({ srtWin });
    } catch (error: unknown) {
      return {
        ...baseStatus("failed", "windows"),
        message: `Windows sandbox status check failed: ${describeError(error)}`,
      };
    }
    if (!status.user.provisioned || !status.user.credPresent) {
      return {
        ...baseStatus("setup-required", "windows"),
        message: windowsInstallInstructions(undefined),
      };
    }
    const wfpInstalled = status.wfp.state === "installed";
    if (!wfpInstalled) {
      return {
        ...baseStatus("setup-required", "windows"),
        message: windowsInstallInstructions(undefined),
      };
    }
    return baseStatus("available", "windows");
  }

  async function execute(request: SandboxedProcessRequest): Promise<SandboxedProcessResult> {
    if (closed) {
      throw new SandboxError("sandbox_configuration_error", "The sandbox backend is closed.");
    }
    if (!request.profile.process.enabled) {
      throw new SandboxError(
        "sandbox_policy_denied",
        `Profile ${request.profile.id} does not allow process execution.`,
      );
    }
    const runtime = await ensureInitialized(request.profile);
    const command = buildCommand(request.executable, request.arguments);
    let wrapped: { argv: string[]; env: NodeJS.ProcessEnv };
    try {
      wrapped = await runtime.wrapWithSandboxArgv(
        command,
        undefined,
        undefined,
        request.signal,
        request.workingDirectory,
      );
    } catch (error: unknown) {
      throw classifyExecutionError(error);
    }
    const startedAt = Date.now();
    const timeoutMs = request.timeoutMs ?? request.profile.process.timeoutMs;
    const stdoutLimitBytes = request.stdoutLimitBytes ?? request.profile.process.maxOutputBytes;
    const stderrLimitBytes = request.stderrLimitBytes ?? request.profile.process.maxOutputBytes;
    const timeoutController = new AbortController();
    let timedOut = false;
    const timeoutTimer = setTimeout(() => {
      timedOut = true;
      timeoutController.abort();
    }, timeoutMs);
    const signals: AbortSignal[] = [timeoutController.signal];
    if (request.signal !== undefined) {
      signals.push(request.signal);
    }
    const child = spawn(wrapped.argv[0] as string, wrapped.argv.slice(1), {
      cwd: request.workingDirectory,
      env: request.environment,
      shell: false,
      stdio: ["ignore", "pipe", "pipe"],
      detached: process.platform !== "win32",
      signal: AbortSignal.any(signals),
    });
    let outputLimited = false;
    const abortTreeListener = () => {
      terminateProcessTree(child);
    };
    for (const signal of signals) {
      signal.addEventListener("abort", abortTreeListener, { once: true });
    }
    const stdoutSink = createOutputSink(stdoutLimitBytes, () => {
      outputLimited = true;
      timeoutController.abort();
    });
    const stderrSink = createOutputSink(stderrLimitBytes, () => {
      outputLimited = true;
      timeoutController.abort();
    });
    const stdoutDecoder = new StringDecoder("utf8");
    const stderrDecoder = new StringDecoder("utf8");
    child.stdout?.on("data", (chunk: Buffer) => {
      stdoutSink.push(chunk);
      if (request.onOutput !== undefined) {
        const text = stdoutDecoder.write(chunk);
        emitChunked(request.onOutput, "stdout", text);
      }
    });
    child.stderr?.on("data", (chunk: Buffer) => {
      stderrSink.push(chunk);
      if (request.onOutput !== undefined) {
        const text = stderrDecoder.write(chunk);
        emitChunked(request.onOutput, "stderr", text);
      }
    });
    const result = await new Promise<SandboxedProcessResult>((resolve, reject) => {
      child.on("error", (error: Error) => {
        reject(
          new SandboxError(
            "sandbox_initialization_failed",
            `Cannot start process: ${error.message}`,
            error,
          ),
        );
      });
      child.on("close", (exitCode: number | null, exitSignal: string | null) => {
        clearTimeout(timeoutTimer);
        for (const signal of signals) {
          signal.removeEventListener("abort", abortTreeListener);
        }
        const violations = collectViolations();
        let status: SandboxedProcessResult["status"];
        if (timedOut) {
          status = "timed-out";
        } else if (outputLimited) {
          status = "output-limit";
        } else if (request.signal?.aborted) {
          status = "cancelled";
        } else if (violations.length > 0) {
          status = "sandbox-denied";
        } else if (exitCode === null) {
          status = "failed";
        } else {
          status = "completed";
        }
        resolve({
          status,
          exitCode,
          signal: exitSignal,
          stdout: stdoutSink.text,
          stderr: stderrSink.text,
          stdoutTruncated: stdoutSink.truncated,
          stderrTruncated: stderrSink.truncated,
          durationMs: Date.now() - startedAt,
          violations,
        });
      });
    });
    if (request.onOutput !== undefined) {
      const stdoutTail = stdoutDecoder.end();
      if (stdoutTail.length > 0) {
        emitChunked(request.onOutput, "stdout", stdoutTail);
      }
      const stderrTail = stderrDecoder.end();
      if (stderrTail.length > 0) {
        emitChunked(request.onOutput, "stderr", stderrTail);
      }
    }
    runtime.cleanupAfterCommand();
    return result;
  }

  async function ensureInitialized(profile: SandboxProfile): Promise<typeof SandboxManager> {
    if (initializedProfile !== undefined) {
      return SandboxManager;
    }
    const config = buildRuntimeConfig(options, profile);
    try {
      await SandboxManager.initialize(config);
    } catch (error: unknown) {
      throw classifyInitializationError(error);
    }
    initializedProfile = profile;
    return SandboxManager;
  }

  async function close(): Promise<void> {
    if (closed) {
      return;
    }
    closed = true;
    try {
      await SandboxManager.reset();
    } catch (error: unknown) {
      throw new SandboxError(
        "sandbox_cleanup_failed",
        `Sandbox cleanup failed: ${describeError(error)}`,
        error,
      );
    }
  }

  return {
    id: ANTHROPIC_SANDBOX_RUNTIME_BACKEND_ID,
    inspect,
    execute,
    close,
  };
}

function baseStatus(state: SandboxBackendStatus["state"], platform: string): SandboxBackendStatus {
  const available = state === "available";
  return {
    backendId: ANTHROPIC_SANDBOX_RUNTIME_BACKEND_ID,
    state,
    platform,
    version: ANTHROPIC_SANDBOX_RUNTIME_VERSION,
    capabilities: {
      filesystemReadRestriction: available,
      filesystemWriteRestriction: available,
      networkRestriction: available,
      processTreeRestriction: available,
      violationReporting: available,
    },
  };
}

function unsupportedStatus(platform: string): SandboxBackendStatus {
  return {
    ...baseStatus("unsupported", platform),
    message: "The Anthropic Sandbox Runtime does not support this platform.",
  };
}

function buildRuntimeConfig(
  options: AnthropicSandboxRuntimeBackendOptions,
  profile: SandboxProfile,
): SandboxRuntimeConfig {
  const writableRoots =
    profile.filesystem.workspaceAccess === "read-write"
      ? [options.workspaceRoot, options.sandboxHome, options.sandboxTemp]
      : [options.sandboxHome, options.sandboxTemp];
  if (options.runRoot !== undefined) {
    writableRoots.push(options.runRoot);
  }
  const readableRoots = [options.workspaceRoot, options.sandboxHome, options.sandboxTemp];
  if (options.runRoot !== undefined) {
    readableRoots.push(options.runRoot);
  }
  return {
    network: {
      allowedDomains: [],
      deniedDomains: [],
    },
    filesystem: {
      denyRead: [`${homedir()}${path.sep}**`],
      allowRead: readableRoots,
      allowWrite: writableRoots,
      denyWrite: protectedPathPatterns(profile, options.workspaceRoot),
    },
    enableWeakerNestedSandbox: false,
    enableWeakerNetworkIsolation: false,
    allowAppleEvents: false,
    windows: {
      srtWin: { path: VENDORED_SRT_WIN_EXE },
    },
  };
}

function protectedPathPatterns(profile: SandboxProfile, workspaceRoot: string): string[] {
  const patterns: string[] = [];
  if (profile.filesystem.protectGitMetadata) {
    patterns.push(`${workspaceRoot}${path.sep}.git${path.sep}**`);
  }
  if (profile.filesystem.protectSolarisMetadata) {
    patterns.push(`${workspaceRoot}${path.sep}.solaris${path.sep}**`);
  }
  if (profile.filesystem.denySensitiveProjectFiles) {
    patterns.push(`${workspaceRoot}${path.sep}.env`);
    patterns.push(`${workspaceRoot}${path.sep}.env.*`);
    patterns.push(`${workspaceRoot}${path.sep}*.pem`);
    patterns.push(`${workspaceRoot}${path.sep}*.key`);
  }
  return patterns;
}

function buildCommand(executable: string, arguments_: readonly string[]): string {
  if (detectPlatform() === "windows") {
    return [quoteWindows(executable), ...arguments_.map(quoteWindows)].join(" ");
  }
  return [quotePosix(executable), ...arguments_.map(quotePosix)].join(" ");
}

function detectPlatform(): "macos" | "linux" | "windows" | "unknown" {
  if (process.platform === "darwin") {
    return "macos";
  }
  if (process.platform === "linux") {
    return "linux";
  }
  if (process.platform === "win32") {
    return "windows";
  }
  return "unknown";
}

function quotePosix(argument: string): string {
  return `'${argument.replace(/'/g, `'\\''`)}'`;
}

function quoteWindows(argument: string): string {
  if (!/[\s"]/.test(argument)) {
    return argument;
  }
  return `"${argument.replace(/"/g, '\\"')}"`;
}

function createOutputSink(
  maxBytes: number,
  onLimitReached: () => void,
): {
  text: string;
  truncated: boolean;
  push(chunk: Buffer): void;
} {
  let text = "";
  let truncated = false;
  let limitReported = false;
  return {
    get text(): string {
      return text;
    },
    get truncated(): boolean {
      return truncated;
    },
    push(chunk: Buffer): void {
      if (truncated) {
        return;
      }
      const remaining = maxBytes - Buffer.byteLength(text, "utf8");
      if (chunk.length > remaining) {
        text += chunk.subarray(0, Math.max(remaining, 0)).toString("utf8");
        truncated = true;
        if (!limitReported) {
          limitReported = true;
          onLimitReached();
        }
      } else {
        text += chunk.toString("utf8");
      }
    },
  };
}

function emitChunked(
  onOutput: (event: { readonly type: "stdout" | "stderr"; readonly text: string }) => void,
  stream: "stdout" | "stderr",
  text: string,
): void {
  for (const chunk of chunkDecodedText(text, COMMAND_LIMITS.maxSingleOutputEventBytes)) {
    if (chunk.length > 0) {
      onOutput({ type: stream, text: chunk });
    }
  }
}

function chunkDecodedText(text: string, maxBytes: number): readonly string[] {
  const chunks: string[] = [];
  let current = "";
  for (const character of text) {
    const next = current + character;
    if (Buffer.byteLength(next, "utf8") > maxBytes && current.length > 0) {
      chunks.push(current);
      current = character;
    } else {
      current = next;
    }
  }
  if (current.length > 0) {
    chunks.push(current);
  }
  return chunks;
}

function terminateProcessTree(child: ChildProcess): void {
  if (child.pid === undefined) {
    return;
  }
  try {
    if (process.platform === "win32") {
      spawn("taskkill", ["/pid", String(child.pid), "/T", "/F"], {
        stdio: "ignore",
        windowsHide: true,
      });
    } else {
      try {
        process.kill(-child.pid, "SIGKILL");
      } catch {
        process.kill(child.pid, "SIGKILL");
      }
    }
  } catch {
    // best-effort tree termination; the abort signal already kills the root
  }
}

function collectViolations(): readonly SandboxViolation[] {
  const violations = SandboxManager.getSandboxViolationStore().getViolations();
  const normalized: SandboxViolation[] = violations.map((violation) => ({
    category: "sandbox",
    summary: truncateSummary(violation.line),
  }));
  SandboxManager.getSandboxViolationStore().clear();
  return normalized;
}

function truncateSummary(text: string): string {
  const limit = 500;
  return text.length > limit ? `${text.slice(0, limit)}...` : text;
}

function classifyInitializationError(error: unknown): SandboxError {
  if (error instanceof SandboxError) {
    return error;
  }
  const message = describeError(error);
  if (message.includes("windows-install") || message.includes("not provisioned")) {
    return new SandboxError("sandbox_setup_required", message, error);
  }
  return new SandboxError("sandbox_initialization_failed", message, error);
}

function classifyExecutionError(error: unknown): SandboxError {
  if (error instanceof SandboxError) {
    return error;
  }
  const message = describeError(error);
  if (message.includes("windows-install") || message.includes("not provisioned")) {
    return new SandboxError("sandbox_setup_required", message, error);
  }
  return new SandboxError("sandbox_execution_denied", message, error);
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unknown sandbox failure occurred.";
}
