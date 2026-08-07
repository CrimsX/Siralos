import type {
  ProcessOutputEvent,
  SandboxBackend,
  SandboxBackendStatus,
  SandboxedProcessRequest,
  SandboxedProcessResult,
} from "@solaris/core";

export interface FakeSandboxBackendOptions {
  readonly status?: SandboxBackendStatus;
  readonly results?: readonly SandboxedProcessResult[];
  readonly inspectError?: Error;
  readonly executeError?: Error;
  /** Output events emitted on `onOutput` before each scripted result. */
  readonly outputs?: readonly ProcessOutputEvent[];
}

export function createFakeSandboxBackend(options: FakeSandboxBackendOptions = {}): {
  backend: SandboxBackend;
  requests: () => SandboxedProcessRequest[];
  closeCalls: () => number;
} {
  const requests: SandboxedProcessRequest[] = [];
  let closeCount = 0;
  let resultIndex = 0;
  const status: SandboxBackendStatus = options.status ?? {
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
  const backend: SandboxBackend = {
    id: "fake-backend",
    inspect(): Promise<SandboxBackendStatus> {
      if (options.inspectError !== undefined) {
        return Promise.reject(options.inspectError);
      }
      return Promise.resolve(status);
    },
    execute(request: SandboxedProcessRequest): Promise<SandboxedProcessResult> {
      requests.push(request);
      if (options.executeError !== undefined) {
        return Promise.reject(options.executeError);
      }
      for (const event of options.outputs ?? []) {
        request.onOutput?.(event);
      }
      const scripted = options.results?.[resultIndex];
      if (scripted !== undefined) {
        resultIndex += 1;
        return Promise.resolve(scripted);
      }
      return Promise.resolve({
        status: "completed",
        exitCode: 0,
        signal: null,
        stdout: "",
        stderr: "",
        stdoutTruncated: false,
        stderrTruncated: false,
        durationMs: 0,
        violations: [],
      });
    },
    close(): Promise<void> {
      closeCount += 1;
      return Promise.resolve();
    },
  };
  return {
    backend,
    requests: () => requests,
    closeCalls: () => closeCount,
  };
}

export function completedResult(
  overrides: Partial<SandboxedProcessResult> = {},
): SandboxedProcessResult {
  return {
    status: "completed",
    exitCode: 0,
    signal: null,
    stdout: "",
    stderr: "",
    stdoutTruncated: false,
    stderrTruncated: false,
    durationMs: 10,
    violations: [],
    ...overrides,
  };
}
