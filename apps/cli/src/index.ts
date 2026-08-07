#!/usr/bin/env node

import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import { createInteractiveApprovalReviewer } from "./approval/approval-reviewer.js";
import { createCliApplication } from "./bootstrap/create-application.js";
import { runSandboxDoctor } from "./bootstrap/sandbox-doctor.js";
import { createInputQueue, type InputQueue } from "./input/input-queue.js";
import {
  createSessionControls,
  runInteractiveSession,
  type SessionControls,
  type SessionIO,
  type SessionInfo,
} from "./interactive-session.js";
import { describeError, formatDoctor, formatHeader, TerminalSanitizer } from "./output.js";

async function main(): Promise<number> {
  const args = process.argv.slice(2);
  if (args.includes("--sandbox-doctor")) {
    const report = await runSandboxDoctor({ includeProbes: args.includes("--run-probes") });
    const sanitizer = new TerminalSanitizer();
    stdout.write(sanitizer.push(formatDoctor(report)) + sanitizer.flush());
    return doctorExitCode(report, args.includes("--run-probes"));
  }
  const readline = createInterface({ input: stdin, output: stdout });
  const controls: SessionControls = createSessionControls();
  readline.on("SIGINT", () => {
    const cancelled = controls.cancelActivePrompt();
    if (!cancelled) {
      readline.close();
      return;
    }
    stdout.write("\n[cancelled; Solaris stays active]\n");
  });
  const lines = readline[Symbol.asyncIterator]();
  // All terminal output funnels through this sanitizer: provider deltas,
  // command output, tool activity, approval prompts, Git output, checkpoint
  // listings, filenames, and error messages are untrusted input.
  const sanitizer = new TerminalSanitizer();
  // One component owns terminal reads: the queue serializes main-loop,
  // approval, and busy-command reads and makes them cancellable.
  const inputQueue: InputQueue = createInputQueue(
    async (): Promise<string | null> => {
      const result = await lines.next();
      return result.done ? null : result.value;
    },
    (text: string): void => {
      stdout.write(sanitizer.push(text));
    },
  );
  const io: SessionIO = {
    ask(prompt: string): Promise<string | null> {
      return inputQueue
        .ask(prompt)
        .then((outcome) => (outcome.kind === "answer" ? outcome.value : null));
    },
    write(text: string): void {
      inputQueue.write(text);
    },
    clear(): void {
      sanitizer.flush();
      stdout.write("\x1b[2J\x1b[H");
    },
  };
  const reviewer = createInteractiveApprovalReviewer(inputQueue);
  const {
    application,
    providerId,
    workspaceRoot,
    tools,
    security,
    sandbox,
    checkpoints,
    git,
    undo,
    runners,
  } = await createCliApplication({ reviewer });
  stdout.write(sanitizer.push(formatHeader(providerId)) + sanitizer.flush());
  const sessionInfo: SessionInfo = {
    workspaceRoot,
    tools,
    security,
    git,
    checkpoints,
    undo,
    runners,
    sandbox,
  };
  const exitCode = await runInteractiveSession(io, application, sessionInfo, controls, inputQueue);
  readline.close();
  stdin.destroy();
  await sandbox.close();
  return exitCode;
}

export function doctorExitCode(
  report: Awaited<ReturnType<typeof runSandboxDoctor>>,
  includeProbes: boolean,
): number {
  if (!includeProbes) {
    return 0;
  }
  if (!report.probesRun) {
    return 3;
  }
  if (report.conformance === null) {
    return 3;
  }
  return report.conformance.failed > 0 ? 1 : 0;
}

main().then(
  (exitCode) => {
    process.exitCode = exitCode;
  },
  (error: unknown) => {
    console.error(`Solaris failed to start: ${describeError(error)}`);
    process.exitCode = 1;
  },
);
