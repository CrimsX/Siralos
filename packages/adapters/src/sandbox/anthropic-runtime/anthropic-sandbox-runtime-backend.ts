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
import { createErrorDescriber } from "../../support/error-message.js";

const describeError = createErrorDescriber("An unknown sandbox failure occurred.");
import { realpathSync } from "node:fs";
import path from "node:path";
import {
  environmentKeyOf,
  isDeniedVariable,
  isProtectedEnvironmentKey,
} from "../../environment/child-environment.js";
import { resolveNpmCli } from "../../process/trusted-executables.js";

export const ANTHROPIC_SANDBOX_RUNTIME_BACKEND_ID = "anthropic-runtime";

export const ANTHROPIC_SANDBOX_RUNTIME_VERSION = "0.0.70";

export interface AnthropicSandboxRuntimeBackendHooks {
  /**
   * Test-only seam: invoked inside the serialized execution queue for each
   * request, after the queued-state checks (closed, aborted, timed-out) and
   * before any SandboxManager interaction. Production callers never set it.
   */
  readonly onExecuteStarted?: (request: SandboxedProcessRequest) => Promise<void> | void;
}

export interface AnthropicSandboxRuntimeBackendOptions {
  /** The active Solaris workspace; granted only when the profile allows it. */
  readonly workspaceRoot: string;
  /**
   * Test-only backend seams. Never used by production callers.
   */
  readonly hooks?: AnthropicSandboxRuntimeBackendHooks;
}

/**
 * The approved host-readable surface for sandboxed commands when the caller
 * does not pin explicit read roots: the exact trusted runner executables and
 * their installation directories plus the minimum system runtime/library
 * paths the selected trusted runner requires. The workspace and the current
 * run directory are granted separately per profile/per request; shared
 * Solaris-owned sandbox directories no longer exist. Everything else on the
 * host is denied.
 *
 * When a request pins `explicitReadRoots`, the caller takes over the
 * identity-bound read surface entirely: the sandboxed child may read only
 * its own run directory, the exact explicit roots, and the minimum system
 * runtime paths — never the workspace, the trusted-runner installation
 * directories, or the npm CLI directories.
 */
export async function hostReadAllowSurface(): Promise<readonly string[]> {
  const system = systemRuntimeReadPaths(detectPlatform());
  const trusted = await trustedRunnerReadPaths();
  return [...system, ...trusted];
}

let cachedTrustedSurface: readonly string[] | undefined;

async function trustedRunnerReadPaths(): Promise<readonly string[]> {
  if (cachedTrustedSurface !== undefined) {
    return cachedTrustedSurface;
  }
  const surface = new Set<string>();
  const nodeExecutable = process.execPath;
  try {
    surface.add(realpathSync(nodeExecutable));
  } catch {
    surface.add(nodeExecutable);
  }
  const nodeDirectory = path.dirname(nodeExecutable);
  surface.add(nodeDirectory);
  surface.add(path.join(nodeDirectory, "..", "lib"));
  const npmCli = await resolveNpmCli();
  if (npmCli.status === "resolved") {
    surface.add(npmCli.cliPath);
    surface.add(path.dirname(npmCli.cliPath));
    surface.add(path.dirname(path.dirname(npmCli.cliPath)));
  }
  cachedTrustedSurface = [...surface];
  return cachedTrustedSurface;
}

/**
 * The minimum system runtime/library paths required by the selected trusted
 * runners. These are machine system locations only — never user data,
 * credentials, logs, caches, or shared scratch regions.
 */
function systemRuntimeReadPaths(platform: ReturnType<typeof detectPlatform>): readonly string[] {
  switch (platform) {
    case "linux":
      return [
        "/bin",
        "/sbin",
        "/usr/bin",
        "/usr/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/lib",
        "/lib32",
        "/lib64",
        "/usr/lib",
        "/usr/lib32",
        "/usr/lib64",
        "/usr/libexec",
        "/usr/local/lib",
        "/etc/ld.so.cache",
        "/etc/ld.so.conf",
        "/etc/ld.so.conf.d",
        "/etc/passwd",
        "/etc/group",
        "/etc/nsswitch.conf",
        "/etc/resolv.conf",
        "/etc/hosts",
        "/etc/localtime",
        "/usr/share/zoneinfo",
      ];
    case "macos":
      return [
        "/bin",
        "/sbin",
        "/usr/bin",
        "/usr/sbin",
        "/usr/local/bin",
        "/usr/lib",
        "/usr/libexec",
        "/usr/local/lib",
        "/System",
        "/usr/share",
        "/usr/share/zoneinfo",
        "/etc/passwd",
        "/etc/group",
        "/etc/hosts",
        "/etc/resolv.conf",
        "/etc/localtime",
        "/private/etc/passwd",
        "/private/etc/group",
        "/private/etc/hosts",
        "/private/etc/resolv.conf",
        "/private/etc/localtime",
      ];
    case "windows":
    case "unknown":
      return [];
  }
}

export function createAnthropicSandboxRuntimeBackend(
  options: AnthropicSandboxRuntimeBackendOptions,
): SandboxBackend {
  let initializedKey: string | undefined;
  let closed = false;
  let transition: Promise<unknown> = Promise.resolve();

  async function inspect(): Promise<SandboxBackendStatus> {
    const platform = detectPlatform();
    if (platform === "unknown" || !SandboxManager.isSupportedPlatform()) {
      return unsupportedStatus(platform);
    }
    if (platform === "windows") {
      // Windows must NEVER report "available": even a fully provisioned,
      // WFP-installed runtime cannot enforce the host-read allowlist there,
      // so process execution is always refused. The windows branch returns
      // only failed / setup-required / degraded states.
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
    // Runtime setup can be complete while the required process boundary is
    // still impossible: per-execution filesystem grants are unsupported and
    // ACL stamping cannot override inherited well-known-group read access,
    // so a deny-by-default host-read allowlist cannot be established.
    // Availability is never claimed and execution is refused; the two
    // conditions (runtime setup and host-read capability) are separate.
    return {
      ...baseStatus("degraded", "windows"),
      message:
        "Windows sandbox runtime setup is complete, but the pinned Sandbox Runtime cannot express a reliable host-read allowlist on Windows (per-execution grants are unsupported and ACL stamping cannot override inherited well-known-group read access). Host-read enforcement is unavailable and process execution is refused. Windows runtime setup and host-read capability are separate conditions: setup can be complete while host-read enforcement is still unavailable and execution is still refused.",
    };
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
    const startedAt = Date.now();
    const deadline = startedAt + (request.timeoutMs ?? request.profile.process.timeoutMs);
    // The COMPLETE lifecycle runs inside the serialized queue: effective-
    // config selection, reset, initialize, wrap, spawn, execution, violation
    // collection, and cleanupAfterCommand all happen one request at a time,
    // so no request can reset or clean the shared SandboxManager out from
    // under another. Queue acquisition races against the request's abort
    // signal and the absolute deadline: a queued request settles promptly
    // without waiting for the active execution.
    return serializedCancellable({
      task: () => executeSerialized(request, startedAt, deadline),
      signal: request.signal,
      deadline,
      settledBeforeStart: () =>
        preStartResult(request.signal?.aborted ? "cancelled" : "timed-out", startedAt),
    });
  }

  async function executeSerialized(
    request: SandboxedProcessRequest,
    startedAt: number,
    deadline: number,
  ): Promise<SandboxedProcessResult> {
    if (closed) {
      throw new SandboxError("sandbox_configuration_error", "The sandbox backend is closed.");
    }
    if (request.signal?.aborted) {
      // The request was cancelled while queued behind another execution;
      // fail closed without starting any process.
      return preStartResult("cancelled", startedAt);
    }
    if (Date.now() >= deadline) {
      // The request timed out while queued behind another execution; fail
      // closed without starting any process.
      return preStartResult("timed-out", startedAt);
    }
    await options.hooks?.onExecuteStarted?.(request);
    // The Windows refusal guards the SandboxManager interaction itself; it
    // runs after the test-only seam (never present in production) so the
    // serialized lifecycle stays observable on every host.
    if (detectPlatform() === "windows") {
      assertHostReadBoundarySupported("windows");
    }
    await ensureInitialized(request.profile);
    const runDirectory = request.runDirectory;
    const customConfig = await buildPerExecutionConfig(
      options,
      request.profile,
      runDirectory,
      request.explicitReadRoots,
    );
    const command = buildCommand(request.executable, request.arguments);
    let wrapped: { argv: string[]; env: NodeJS.ProcessEnv };
    try {
      wrapped = await SandboxManager.wrapWithSandboxArgv(
        command,
        undefined,
        customConfig,
        request.signal,
        request.workingDirectory,
      );
    } catch (error: unknown) {
      throw classifyExecutionError(error);
    }
    let environment: Readonly<Record<string, string>>;
    try {
      environment = mergeWrapperEnvironment(request.environment, wrapped.env);
    } catch (error: unknown) {
      throw classifyExecutionError(error);
    }
    // The total timeout is the ABSOLUTE deadline from request entry: time
    // spent queued behind other executions counts toward it, so the child
    // never receives a fresh full timeout after dequeue. A request that
    // expired while queued already returned a timed-out result above.
    const remainingMs = deadline - Date.now();
    const timeoutMs = Math.max(0, remainingMs);
    const stdoutLimitBytes = request.stdoutLimitBytes ?? request.profile.process.maxOutputBytes;
    const stderrLimitBytes = request.stderrLimitBytes ?? request.profile.process.maxOutputBytes;
    const timeoutController = new AbortController();
    let timedOut = false;
    let outputLimited = false;
    const timeoutTimer = setTimeout(() => {
      timedOut = true;
      timeoutController.abort();
    }, timeoutMs);
    const signals: AbortSignal[] = [timeoutController.signal];
    if (request.signal !== undefined) {
      signals.push(request.signal);
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
    let result: SandboxedProcessResult;
    try {
      const child = spawn(wrapped.argv[0] as string, wrapped.argv.slice(1), {
        cwd: request.workingDirectory,
        env: environment,
        shell: false,
        stdio: ["ignore", "pipe", "pipe"],
        detached: process.platform !== "win32",
        signal: AbortSignal.any(signals),
      });
      const abortTreeListener = () => {
        terminateProcessTree(child);
      };
      for (const signal of signals) {
        signal.addEventListener("abort", abortTreeListener, { once: true });
      }
      child.stdout?.on("data", (chunk: Buffer) => {
        stdoutSink.push(chunk);
        if (request.onOutput !== undefined && !outputLimited && !timedOut) {
          emitChunked(request.onOutput, "stdout", stdoutDecoder.write(chunk));
        } else {
          stdoutDecoder.write(chunk);
        }
      });
      child.stderr?.on("data", (chunk: Buffer) => {
        stderrSink.push(chunk);
        if (request.onOutput !== undefined && !outputLimited && !timedOut) {
          emitChunked(request.onOutput, "stderr", stderrDecoder.write(chunk));
        } else {
          stderrDecoder.write(chunk);
        }
      });
      result = await new Promise<SandboxedProcessResult>((resolve, reject) => {
        child.on("error", (error: Error) => {
          clearTimeout(timeoutTimer);
          for (const signal of signals) {
            signal.removeEventListener("abort", abortTreeListener);
          }
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
      if (request.onOutput !== undefined && !outputLimited && !timedOut) {
        const stdoutTail = stdoutDecoder.end();
        if (stdoutTail.length > 0) {
          emitChunked(request.onOutput, "stdout", stdoutTail);
        }
        const stderrTail = stderrDecoder.end();
        if (stderrTail.length > 0) {
          emitChunked(request.onOutput, "stderr", stderrTail);
        }
      } else {
        stdoutDecoder.end();
        stderrDecoder.end();
      }
    } finally {
      clearTimeout(timeoutTimer);
      try {
        SandboxManager.cleanupAfterCommand();
      } catch {
        // best-effort runtime cleanup; never masks the command result
      }
    }
    return result;
  }

  async function ensureInitialized(profile: SandboxProfile): Promise<void> {
    const key = effectiveConfigKey(options, profile);
    if (initializedKey === key) {
      return;
    }
    if (initializedKey !== undefined) {
      try {
        await SandboxManager.reset();
      } catch (error: unknown) {
        throw new SandboxError(
          "sandbox_cleanup_failed",
          `The previous sandbox configuration could not be torn down: ${describeError(error)}`,
          error,
        );
      }
    }
    const config = await buildSessionConfig(options, profile);
    try {
      await SandboxManager.initialize(config);
    } catch (error: unknown) {
      throw classifyInitializationError(error);
    }
    // Only report the profile as active after initialization succeeded.
    initializedKey = key;
  }

  async function close(): Promise<void> {
    if (closed) {
      return;
    }
    // Mark closed first so a failed reset can never leave a half-reset
    // manager serving requests: every subsequent execute rejects.
    closed = true;
    // Drain the serialization queue (active and queued executions settle
    // first), then reset. The reset task is enqueued behind everything.
    await serialized(async () => {
      try {
        await SandboxManager.reset();
      } catch (error: unknown) {
        throw new SandboxError(
          "sandbox_cleanup_failed",
          `Sandbox cleanup failed: ${describeError(error)}`,
          error,
        );
      }
    });
  }

  function serialized<T>(task: () => Promise<T>): Promise<T> {
    const run = transition.then(task, task);
    transition = run.then(
      () => undefined,
      () => undefined,
    );
    return run;
  }

  /**
   * Queue acquisition that races against the request's abort signal and the
   * absolute deadline. A queued request settles promptly — with
   * `settledBeforeStart()` — the moment its signal aborts or its deadline
   * expires, without waiting for the active execution to finish; when its
   * queue slot arrives the task is skipped entirely, so a settled request
   * never starts anything and never disturbs the lifecycle of other
   * requests. `tail` progression is preserved by the skipped slot.
   */
  function serializedCancellable<T>(options: {
    readonly task: () => Promise<T>;
    readonly signal?: AbortSignal | undefined;
    readonly deadline?: number | undefined;
    readonly settledBeforeStart: () => T;
  }): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      let settled = false;
      let running = false;
      const cleanup = (): void => {
        options.signal?.removeEventListener("abort", onAbort);
        if (deadlineTimer !== undefined) {
          clearTimeout(deadlineTimer);
        }
      };
      const finish = (value: T | Promise<T>): void => {
        if (settled) {
          return;
        }
        settled = true;
        cleanup();
        Promise.resolve(value).then(resolve, reject);
      };
      const onAbort = (): void => {
        if (!running) {
          finish(options.settledBeforeStart());
        }
      };
      const deadlineTimer =
        options.deadline === undefined
          ? undefined
          : setTimeout(
              () => {
                if (!running) {
                  finish(options.settledBeforeStart());
                }
              },
              Math.max(0, options.deadline - Date.now()),
            );
      options.signal?.addEventListener("abort", onAbort, { once: true });
      if (options.signal?.aborted) {
        finish(options.settledBeforeStart());
        return;
      }
      if (options.deadline !== undefined && Date.now() >= options.deadline) {
        finish(options.settledBeforeStart());
        return;
      }
      const run = transition.then(
        () => {
          if (settled) {
            return undefined;
          }
          running = true;
          return options.task();
        },
        () => {
          if (settled) {
            return undefined;
          }
          running = true;
          return options.task();
        },
      );
      transition = run.then(
        () => undefined,
        () => undefined,
      );
      run.then(
        (value) => finish(value as T),
        (error: unknown) => {
          if (!settled) {
            settled = true;
            cleanup();
            reject(error instanceof Error ? error : new Error(describeError(error)));
          }
        },
      );
    });
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
  // Windows can never claim host-read enforcement, and no caller may pass
  // "available" for Windows: the inspect windows branch returns only failed,
  // setup-required, or degraded states.
  const readRestrictionAvailable = available && platform !== "windows";
  return {
    backendId: ANTHROPIC_SANDBOX_RUNTIME_BACKEND_ID,
    state,
    platform,
    version: ANTHROPIC_SANDBOX_RUNTIME_VERSION,
    capabilities: {
      filesystemReadRestriction: readRestrictionAvailable,
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

async function buildSessionConfig(
  options: AnthropicSandboxRuntimeBackendOptions,
  profile: SandboxProfile,
): Promise<SandboxRuntimeConfig> {
  const surface = await hostReadAllowSurface();
  return {
    network: {
      allowedDomains: [],
      deniedDomains: [],
    },
    filesystem: {
      denyRead: hostReadBoundaryPatterns(),
      allowRead: [...profileReadRoots(options, profile), ...surface],
      allowWrite: profileWriteRoots(options, profile),
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

/**
 * Builds the per-execution sandbox configuration for one command run. The
 * command may access ONLY:
 *   - its own run directory (home/temp/cache/script-cache are its children;
 *     the environment already names runDirectory/home and runDirectory/tmp),
 *   - the explicit read roots when the caller pins them,
 *   - the workspace when the exact profile allows it,
 *   - the minimum system runtime surface.
 * Previous and concurrent run directories are never granted. When
 * `explicitReadRoots` is present, the readable roots are exactly
 * [runDirectory, ...explicitReadRoots, systemRuntimeReadPaths(platform)]:
 * the caller takes over the identity-bound read surface, so the workspace
 * and the broad trusted-runner surfaces (Node installation, npm CLI,
 * /usr/local/bin-style directories) are NOT granted — the caller adds any
 * path the child needs to the explicit list. When absent, the profile
 * decides the workspace and the trusted-runner surface remains available.
 * Nothing outside the run directory is writable by any profile except the
 * workspace under a read-write profile.
 */
export async function buildPerExecutionConfig(
  options: AnthropicSandboxRuntimeBackendOptions,
  profile: SandboxProfile,
  runDirectory: string | undefined,
  explicitReadRoots?: readonly string[],
): Promise<Partial<SandboxRuntimeConfig>> {
  const readableRoots = profileReadRoots(options, profile, explicitReadRoots);
  if (runDirectory !== undefined) {
    readableRoots.push(runDirectory);
  }
  if (explicitReadRoots !== undefined) {
    readableRoots.push(...systemRuntimeReadPaths(detectPlatform()));
  } else {
    readableRoots.push(...(await hostReadAllowSurface()));
  }
  const writableRoots = profileWriteRoots(options, profile);
  if (runDirectory !== undefined) {
    writableRoots.push(runDirectory);
  }
  return {
    network: {
      allowedDomains: [],
      deniedDomains: [],
    },
    filesystem: {
      denyRead: hostReadBoundaryPatterns(),
      allowRead: readableRoots,
      allowWrite: writableRoots,
      denyWrite: protectedPathPatterns(profile, options.workspaceRoot),
    },
    enableWeakerNestedSandbox: false,
    enableWeakerNetworkIsolation: false,
    allowAppleEvents: false,
  };
}

/**
 * Readable roots for the profile: the source workspace only when the
 * profile does not exclude it and the caller has not pinned explicit read
 * roots (with explicit roots the caller owns the entire read surface).
 * Shared sandbox directories no longer exist and are never granted.
 */
function profileReadRoots(
  options: AnthropicSandboxRuntimeBackendOptions,
  profile: SandboxProfile,
  explicitReadRoots?: readonly string[],
): string[] {
  if (explicitReadRoots !== undefined) {
    return [...explicitReadRoots];
  }
  const readableRoots: string[] = [];
  if (profile.filesystem.excludeWorkspaceRead !== true) {
    readableRoots.push(options.workspaceRoot);
  }
  return readableRoots;
}

function profileWriteRoots(
  options: AnthropicSandboxRuntimeBackendOptions,
  profile: SandboxProfile,
): string[] {
  return profile.filesystem.workspaceAccess === "read-write" ? [options.workspaceRoot] : [];
}

/**
 * The effective-configuration key: every profile-derived part of the runtime
 * configuration that changes the sandbox's effective behavior. Requests are
 * never executed under a previously initialized broader configuration; when
 * the key changes the manager is reset and reinitialized before use.
 */
export function effectiveConfigKey(
  options: AnthropicSandboxRuntimeBackendOptions,
  profile: SandboxProfile,
): string {
  return JSON.stringify({
    workspaceAccess: profile.filesystem.workspaceAccess,
    protectGitMetadata: profile.filesystem.protectGitMetadata,
    protectSolarisMetadata: profile.filesystem.protectSolarisMetadata,
    denySensitiveProjectFiles: profile.filesystem.denySensitiveProjectFiles,
    excludeWorkspaceRead: profile.filesystem.excludeWorkspaceRead,
    processEnabled: profile.process.enabled,
    workspaceRoot: options.workspaceRoot,
  });
}

/**
 * The host-read boundary is an allowlist, not a blocklist. The runtime's
 * read model allows everything by default, so the whole host root is denied
 * first and only the approved surface is re-allowed: the active workspace
 * (when the profile allows it), the current command's private run
 * directory, the exact trusted runner executables, and the minimum system
 * runtime/library paths the selected trusted runner requires. Nothing else —
 * other user profiles, shared temporary areas, mounted volumes, other
 * drives, system configuration beyond the listed runtime paths, service
 * data, logs, caches, or credential locations — is readable. Requests that
 * pin explicit read roots replace the workspace and trusted-runner surface
 * with their exact identity-bound list.
 */
export function hostReadBoundaryPatterns(
  platform: ReturnType<typeof detectPlatform> = detectPlatform(),
): string[] {
  switch (platform) {
    case "unknown":
      return [];
    case "windows":
      // Windows execution is refused (the runtime cannot express a reliable
      // allowlist there); no partial deny surface is claimed.
      return [];
    case "macos":
    case "linux":
      return ["/"];
  }
}

/**
 * Refuses process execution on platforms where the pinned runtime cannot
 * express the deny-by-default host-read allowlist. On Windows per-execution
 * filesystem grants are unsupported and ACL stamping cannot override
 * inherited well-known-group read access, so a reliable allowlist cannot be
 * established; execution fails closed instead of claiming a boundary.
 */
export function assertHostReadBoundarySupported(platform: string): void {
  if (platform === "windows") {
    throw new SandboxError(
      "sandbox_configuration_error",
      "The pinned Sandbox Runtime cannot enforce a deny-by-default host-read allowlist on Windows; process execution is refused. Report this platform as unable to enforce the host-read boundary.",
    );
  }
}

function isPathInside(root: string, target: string): boolean {
  const rootPrefix = root.endsWith(path.sep) ? root : `${root}${path.sep}`;
  return target === root || target.startsWith(rootPrefix);
}

export function isWithinHostReadAllowSurface(
  candidate: string,
  surface: readonly string[],
): boolean {
  return surface.some((root) => isPathInside(root, candidate));
}

/**
 * Merges the sandbox wrapper's runtime-required environment into Solaris's
 * minimal allowlisted environment. The wrapper (Sandbox Runtime) returns the
 * environment its wrapped invocation needs; only that explicit set is
 * merged — never the host environment wholesale. Solaris-controlled keys
 * (HOME, USERPROFILE, TEMP, TMP, TMPDIR, compared case-insensitively on
 * Windows) can never be replaced or introduced by the wrapper: a collision
 * resolves to the Solaris-controlled value, and a protected key absent from
 * the base fails closed. Wrapper-only keys are added, keys matching the
 * credential/proxy/Node-injection deny patterns fail closed, and keys are
 * normalized through the same case-insensitive Windows comparison used by
 * `buildChildEnvironment`, so duplicate spellings cannot bypass filtering
 * (canonical casing wins on Windows).
 */
export function mergeWrapperEnvironment(
  base: Readonly<Record<string, string>>,
  wrapperEnvironment: Readonly<Record<string, string | undefined>>,
  platform: NodeJS.Platform = process.platform,
): Readonly<Record<string, string>> {
  const keyOf = (name: string): string => environmentKeyOf(name, platform);
  const merged: Record<string, string> = {};
  const baseKeys = new Map<string, string>();
  for (const [name, value] of Object.entries(base)) {
    const normalized = keyOf(name);
    if (baseKeys.has(normalized)) {
      continue;
    }
    baseKeys.set(normalized, name);
    merged[name] = value;
  }
  for (const [name, value] of Object.entries(wrapperEnvironment)) {
    if (value === undefined) {
      continue;
    }
    if (isDeniedVariable(name)) {
      throw new SandboxError(
        "sandbox_configuration_error",
        `The sandbox wrapper requires environment variable ${name}, which Solaris denies; refusing to execute.`,
      );
    }
    if (isProtectedEnvironmentKey(name, platform)) {
      if (baseKeys.has(keyOf(name))) {
        // The Solaris-controlled value already in the base wins; the
        // wrapper can never replace it.
        continue;
      }
      throw new SandboxError(
        "sandbox_configuration_error",
        `The sandbox wrapper requires environment variable ${name}, which Solaris controls; refusing to execute.`,
      );
    }
    if (baseKeys.has(keyOf(name))) {
      continue;
    }
    merged[name] = value;
  }
  return merged;
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

/**
 * Output sink that accounts the hard limit on raw bytes, keeps decoded text
 * valid across child-process buffer boundaries, and never truncates inside a
 * multibyte sequence. The limit is enforced within the crossing chunk, so a
 * single large OS buffer cannot bypass it.
 */
export function createOutputSink(
  maxBytes: number,
  onLimitReached: () => void,
): {
  text: string;
  truncated: boolean;
  push(chunk: Buffer): void;
} {
  const decoder = new StringDecoder("utf8");
  let text = "";
  let totalBytes = 0;
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
      const remaining = maxBytes - totalBytes;
      if (chunk.length > remaining) {
        // Keep only the bytes that fit; the decoder drops any dangling
        // partial sequence instead of emitting a replacement character.
        text += decoder.write(chunk.subarray(0, Math.max(remaining, 0)));
        truncated = true;
        if (!limitReported) {
          limitReported = true;
          onLimitReached();
        }
      } else {
        totalBytes += chunk.length;
        text += decoder.write(chunk);
      }
    },
  };
}

export function emitChunked(
  onOutput: (event: { readonly type: "stdout" | "stderr"; readonly text: string }) => void,
  stream: "stdout" | "stderr",
  text: string,
): void {
  for (const chunk of chunkDecodedText(text, COMMAND_LIMITS.maxSingleOutputEventBytes)) {
    if (chunk.length > 0) {
      try {
        onOutput({ type: stream, text: chunk });
      } catch {
        // A failing output callback must never crash Solaris or leak a
        // running child; the command continues and the callback is skipped.
      }
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
  try {
    const violations = SandboxManager.getSandboxViolationStore().getViolations();
    const normalized: SandboxViolation[] = violations.map((violation) => ({
      category: "sandbox",
      summary: truncateSummary(violation.line),
    }));
    SandboxManager.getSandboxViolationStore().clear();
    return normalized;
  } catch {
    // Violation collection must never corrupt the command result: a failing
    // store is reported as "no violations" instead of an error.
    return [];
  }
}

/**
 * Result for a request that failed closed before any process started: it
 * was cancelled or timed out while queued behind another execution.
 */
function preStartResult(
  status: "cancelled" | "timed-out",
  startedAt: number,
): SandboxedProcessResult {
  return {
    status,
    exitCode: null,
    signal: null,
    stdout: "",
    stderr: "",
    stdoutTruncated: false,
    stderrTruncated: false,
    durationMs: Date.now() - startedAt,
    violations: [],
  };
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
