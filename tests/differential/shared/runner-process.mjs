/** Bounded lifecycle supervision for R2 reference/candidate runner processes. */
import { spawn, spawnSync } from "node:child_process";
import { RUNNER_OUTCOME } from "./protocol.mjs";

export const RUNNER_PROCESS_LIMITS = Object.freeze({
  diagnosticsBytes: 64 * 1024,
  runnerTimeoutMs: 30_000,
  buildTimeoutMs: 300_000,
  terminationGraceMs: 2_000,
});

const DIAGNOSTIC_PREFIX = "SIRALOS_HARNESS_ERROR ";

function boundedText(chunks, maximumBytes) {
  const bytes = Buffer.concat(chunks);
  return bytes.subarray(0, maximumBytes).toString("utf8");
}

function parseHarnessDiagnostic(stderr) {
  const line = stderr.split(/\r?\n/u).find((candidate) => candidate.startsWith(DIAGNOSTIC_PREFIX));
  if (line === undefined) return null;
  try {
    const value = JSON.parse(line.slice(DIAGNOSTIC_PREFIX.length));
    if (
      value !== null &&
      typeof value === "object" &&
      typeof value.category === "string" &&
      typeof value.code === "string" &&
      typeof value.message === "string"
    ) {
      return value;
    }
  } catch {
    // A malformed machine diagnostic is classified as a process crash below.
  }
  return null;
}

function terminateTree(child) {
  if (child.pid === undefined) return;
  if (process.platform === "win32") {
    try {
      child.kill();
    } catch {
      // Continue to the descendant-aware taskkill fallback.
    }
    spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
      stdio: "ignore",
      timeout: RUNNER_PROCESS_LIMITS.terminationGraceMs,
      windowsHide: true,
    });
    return;
  }
  try {
    process.kill(-child.pid, "SIGKILL");
  } catch {
    try {
      child.kill("SIGKILL");
    } catch {
      // The process already exited between observation and termination.
    }
  }
}

/**
 * Run one isolated harness process with bounded output and a hard deadline.
 * Product failure is never returned here: it belongs in a successful runner's
 * protocol document.
 */
export function superviseRunner({
  implementation,
  scenarioId,
  command,
  args = [],
  cwd,
  env = process.env,
  timeoutMs = RUNNER_PROCESS_LIMITS.runnerTimeoutMs,
  diagnosticsBytes = RUNNER_PROCESS_LIMITS.diagnosticsBytes,
}) {
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1) {
    throw new Error("runner timeout must be a positive safe integer");
  }
  return new Promise((resolve) => {
    const stdout = [];
    const stderr = [];
    let stdoutBytes = 0;
    let stderrBytes = 0;
    let terminal = false;
    let limitExceeded = false;
    let timedOut = false;
    let deadline;

    const child = spawn(command, args, {
      cwd,
      env,
      detached: process.platform !== "win32",
      shell: false,
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });

    const finish = (result) => {
      if (terminal) return;
      terminal = true;
      if (deadline !== undefined) clearTimeout(deadline);
      resolve({
        implementation,
        scenarioId,
        exitCode: null,
        signal: null,
        stdout: boundedText(stdout, diagnosticsBytes),
        stderr: boundedText(stderr, diagnosticsBytes),
        ...result,
      });
    };

    const collect = (chunks, chunk, stream) => {
      if (terminal) return;
      const current = stream === "stdout" ? stdoutBytes : stderrBytes;
      const remaining = Math.max(0, diagnosticsBytes + 1 - current);
      if (remaining > 0) chunks.push(chunk.subarray(0, remaining));
      if (stream === "stdout") stdoutBytes += chunk.length;
      else stderrBytes += chunk.length;
      if (stdoutBytes > diagnosticsBytes || stderrBytes > diagnosticsBytes) {
        limitExceeded = true;
        terminateTree(child);
        finish({
          outcome: RUNNER_OUTCOME.PROTOCOL_ERROR,
          category: "RUNNER_OUTPUT_LIMIT",
          code: "DIAGNOSTICS_LIMIT_EXCEEDED",
          message: `${implementation} exceeded bounded diagnostics for fixture ${scenarioId}`,
        });
      }
    };

    child.stdout.on("data", (chunk) => collect(stdout, chunk, "stdout"));
    child.stderr.on("data", (chunk) => collect(stderr, chunk, "stderr"));
    child.once("error", (error) => {
      finish({
        outcome: RUNNER_OUTCOME.HARNESS_ERROR,
        category: "RUNNER_SPAWN_FAILURE",
        code: error.code ?? "SPAWN_FAILED",
        message: `could not spawn ${implementation} runner`,
      });
    });
    child.once("close", (exitCode, signal) => {
      if (timedOut) {
        finish({
          outcome: RUNNER_OUTCOME.TIMED_OUT,
          category: "RUNNER_TIMEOUT",
          code: "RUNNER_TIMED_OUT",
          message: `${implementation} timed out for fixture ${scenarioId}`,
          exitCode,
          signal,
        });
        return;
      }
      if (limitExceeded) {
        finish({
          outcome: RUNNER_OUTCOME.PROTOCOL_ERROR,
          category: "RUNNER_OUTPUT_LIMIT",
          code: "DIAGNOSTICS_LIMIT_EXCEEDED",
          message: `${implementation} exceeded bounded diagnostics for fixture ${scenarioId}`,
          exitCode,
          signal,
        });
        return;
      }
      if (signal !== null || exitCode !== 0) {
        const stderrText = boundedText(stderr, diagnosticsBytes);
        const diagnostic = parseHarnessDiagnostic(stderrText);
        if (exitCode === 2 && signal === null && diagnostic !== null) {
          finish({
            outcome: RUNNER_OUTCOME.HARNESS_ERROR,
            ...diagnostic,
            exitCode,
            signal,
          });
        } else {
          finish({
            outcome: RUNNER_OUTCOME.PROCESS_CRASHED,
            category: "RUNNER_PROCESS_CRASH",
            code: signal === null ? "NONZERO_EXIT" : "SIGNAL_EXIT",
            message: `${implementation} process crashed for fixture ${scenarioId}`,
            exitCode,
            signal,
          });
        }
        return;
      }
      finish({
        outcome: RUNNER_OUTCOME.COMPLETED,
        category: null,
        code: null,
        message: null,
        exitCode,
        signal,
      });
    });

    deadline = setTimeout(() => {
      timedOut = true;
      terminateTree(child);
      finish({
        outcome: RUNNER_OUTCOME.TIMED_OUT,
        category: "RUNNER_TIMEOUT",
        code: "RUNNER_TIMED_OUT",
        message: `${implementation} timed out for fixture ${scenarioId}`,
      });
    }, timeoutMs);
    deadline.unref?.();
  });
}
