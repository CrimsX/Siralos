#!/usr/bin/env node

import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import { createInteractiveApprovalReviewer } from "./approval/approval-reviewer.js";
import { createCliApplication } from "./bootstrap/create-application.js";
import { godotDoctorExitCode, runGodotDoctor } from "./bootstrap/godot-doctor.js";
import { doctorExitCode, runSandboxDoctor } from "./bootstrap/sandbox-doctor.js";
import { createInputQueue, type InputQueue } from "./input/input-queue.js";
import {
  createSessionControls,
  runInteractiveSession,
  type SessionControls,
  type SessionIO,
  type SessionInfo,
} from "./interactive-session.js";
import {
  describeError,
  formatDoctor,
  formatGodotDoctor,
  formatHeader,
  TerminalSanitizer,
} from "./output.js";

function optionValue(args: readonly string[], name: string): string | undefined {
  const index = args.indexOf(name);
  if (index < 0) {
    return undefined;
  }
  return args[index + 1];
}

async function main(): Promise<number> {
  const args = process.argv.slice(2);
  if (args.includes("--sandbox-doctor")) {
    const report = await runSandboxDoctor({ includeProbes: args.includes("--run-probes") });
    const sanitizer = new TerminalSanitizer();
    stdout.write(sanitizer.push(formatDoctor(report)) + sanitizer.flush());
    return doctorExitCode(report, args.includes("--run-probes"));
  }
  if (args.includes("--godot-doctor")) {
    const godotPath = optionValue(args, "--godot-path");
    const godotInstallation = optionValue(args, "--godot-installation");
    const recoveryProbeRequested = args.includes("--recovery-probe");
    const report = await runGodotDoctor({
      ...(godotPath === undefined ? {} : { godotPath }),
      ...(godotInstallation === undefined ? {} : { godotInstallation }),
      recoveryProbeRequested,
    });
    const sanitizer = new TerminalSanitizer();
    stdout.write(sanitizer.push(formatGodotDoctor(report)) + sanitizer.flush());
    return godotDoctorExitCode(report, recoveryProbeRequested);
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
  const godotPath = optionValue(args, "--godot-path");
  const godotInstallation = optionValue(args, "--godot-installation");
  const {
    application,
    providerId,
    workspaceRoot,
    tasks,
    taskSources,
    projection,
    revisions,
    workspaceRead,
    tools,
    security,
    sandbox,
    checkpoints,
    git,
    godot,
    godotProbe,
    knowledge,
    diagnostics,
    language,
    development,
    undo,
    runners,
  } = await createCliApplication({
    reviewer,
    ...(godotPath === undefined ? {} : { godotPath }),
    ...(godotInstallation === undefined ? {} : { godotInstallation }),
  });
  stdout.write(sanitizer.push(formatHeader(providerId)) + sanitizer.flush());
  const sessionInfo: SessionInfo = {
    workspaceRoot,
    tasks,
    taskSources,
    projection,
    revisions,
    workspaceRead,
    tools,
    security,
    git,
    godot,
    godotProbe,
    knowledge,
    diagnostics,
    language,
    development,
    reviewer,
    checkpoints,
    undo,
    runners,
    sandbox,
  };
  const exitCode = await runInteractiveSession(io, application, sessionInfo, controls, inputQueue);
  godotProbe.disposeAll();
  diagnostics.disposeAll();
  await language.closeAll();
  await development.close();
  readline.close();
  stdin.destroy();
  await sandbox.close();
  return exitCode;
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
