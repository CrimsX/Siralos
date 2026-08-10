#!/usr/bin/env node

import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import { createInteractiveApprovalReviewer } from "./approval/approval-reviewer.js";
import { createCliApplication } from "./bootstrap/create-application.js";
import { createCliDoctor, isDoctorArea } from "./bootstrap/doctor.js";
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
import { doctorExitCodeFor, toSafeReport } from "@solaris/core";
import {
  describeError,
  formatDoctor,
  formatGodotDoctor,
  formatHeader,
  formatSafeDoctorReport,
  formatSelfReference,
  formatSolarisDoctorReport,
  TerminalSanitizer,
} from "./output.js";

function optionValue(args: readonly string[], name: string): string | undefined {
  const index = args.indexOf(name);
  if (index < 0) {
    return undefined;
  }
  return args[index + 1];
}

/** `--doctor [area]`: the next argv token is the area only when it is not a flag. */
function doctorAreaArg(args: readonly string[]): string | undefined {
  const index = args.indexOf("--doctor");
  const next = args[index + 1];
  if (next === undefined || next.startsWith("--")) {
    return undefined;
  }
  return next;
}

/**
 * Standalone `--doctor` / `--self` (Stage 3 milestone 6). One
 * implementation backs both the standalone flags and the interactive
 * `/doctor` command (same core CapabilityDoctor + sources).
 *
 * Exit codes: 0 = no diagnostic failures, 1 = one or more failures,
 * 2 = doctor invocation/infrastructure failure (unknown area, or the
 * doctor could not run). Warnings never fail.
 */
async function runStandaloneDoctor(args: readonly string[]): Promise<number> {
  if (args.includes("--self")) {
    const cliApp = await createCliApplication({});
    const sanitizer = new TerminalSanitizer();
    try {
      stdout.write(sanitizer.push(formatSelfReference(cliApp.selfReference)) + sanitizer.flush());
      return 0;
    } finally {
      cliApp.close();
      await cliApp.sandbox.close();
    }
  }
  const areaArg = doctorAreaArg(args);
  if (areaArg !== undefined && !isDoctorArea(areaArg)) {
    stdout.write(
      `Unknown doctor area: ${areaArg}. Areas: runtime, configuration, providers, sandbox, workspace, godot, project, references, research, capabilities.\n`,
    );
    return 2;
  }
  const cliApp = await createCliApplication({});
  const sanitizer = new TerminalSanitizer();
  try {
    const doctor = createCliDoctor({
      workspaceRoot: cliApp.workspaceRoot,
      configPath: cliApp.configPath,
      policy: cliApp.policy,
      profile: cliApp.profile,
      sandbox: cliApp.sandbox,
      provider: cliApp.provider,
      godot: cliApp.godot,
      references: cliApp.references,
      referenceConfigError: cliApp.referenceConfigError,
      research: cliApp.research,
      researchSources: cliApp.researchSources,
      tasks: cliApp.tasks,
      taskSources: cliApp.taskSources,
      git: cliApp.git,
      checkpoints: cliApp.checkpoints,
      tools: cliApp.tools,
      mode: "generic",
    });
    const report = await doctor.inspect({
      ...(areaArg === undefined ? {} : { areas: [areaArg] }),
    });
    const safe = args.includes("--report-safe");
    const json = args.includes("--json");
    if (safe && json) {
      stdout.write(
        sanitizer.push(`${JSON.stringify(toSafeReport(report), null, 2)}\n`) + sanitizer.flush(),
      );
    } else if (safe) {
      stdout.write(
        sanitizer.push(formatSafeDoctorReport(toSafeReport(report))) + sanitizer.flush(),
      );
    } else if (json) {
      stdout.write(sanitizer.push(`${JSON.stringify(report, null, 2)}\n`) + sanitizer.flush());
    } else {
      stdout.write(sanitizer.push(formatSolarisDoctorReport(report)) + sanitizer.flush());
    }
    return doctorExitCodeFor(report);
  } catch (error: unknown) {
    stdout.write(`Solaris doctor failed to run: ${describeError(error)}\n`);
    return 2;
  } finally {
    cliApp.close();
    await cliApp.sandbox.close();
  }
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
  if (args.includes("--doctor") || args.includes("--self")) {
    return runStandaloneDoctor(args);
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
  const cliApp = await createCliApplication({
    reviewer,
    ...(godotPath === undefined ? {} : { godotPath }),
    ...(godotInstallation === undefined ? {} : { godotInstallation }),
  });
  const {
    application,
    providerId,
    workspaceRoot,
    configPath,
    policy,
    profile,
    provider,
    selfReference,
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
    instructions,
    projectKnowledge,
    references,
    referenceMaterializer,
    referenceConfigError,
    research,
    researchSources,
  } = cliApp;
  stdout.write(sanitizer.push(formatHeader(providerId)) + sanitizer.flush());
  const sessionInfo: SessionInfo = {
    workspaceRoot,
    configPath,
    policy,
    profile,
    provider,
    selfReference,
    tasks,
    taskSources,
    projection,
    revisions,
    workspaceRead,
    instructions,
    projectKnowledge,
    references,
    referenceMaterializer,
    referenceConfigError,
    research,
    researchSources,
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
  cliApp.close();
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
