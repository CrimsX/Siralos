import { doctorExitCodeFor, toSafeReport } from "@siralos/core";
import { createCliApplication } from "./create-application.js";
import { createCliDoctor, isDoctorArea } from "./doctor.js";
import {
  describeError,
  formatSafeDoctorReport,
  formatSelfReference,
  formatSiralosDoctorReport,
  TerminalSanitizer,
} from "../output.js";

/**
 * Standalone doctor/self CLI entry (Stage 3 milestone 6), extracted from
 * the process entry so the argv boundary (exit codes 0/1/2, flag parsing,
 * output selection) is unit-testable without spawning the interactive
 * session. One implementation backs both the standalone flags and the
 * interactive `/doctor` command (same core CapabilityDoctor + sources).
 *
 * Exit codes: 0 = no diagnostic failures, 1 = one or more failures,
 * 2 = doctor invocation/infrastructure failure (unknown area, or the
 * doctor could not run). Warnings never fail.
 */

export type DoctorCliOutput = (text: string) => void;

export function doctorAreaArg(args: readonly string[]): string | undefined {
  const index = args.indexOf("--doctor");
  const next = args[index + 1];
  if (next === undefined || next.startsWith("--")) {
    return undefined;
  }
  return next;
}

export async function runDoctorCli(
  args: readonly string[],
  write: DoctorCliOutput = (text) => process.stdout.write(text),
): Promise<number> {
  if (args.includes("--self")) {
    const cliApp = await createCliApplication({});
    const sanitizer = new TerminalSanitizer();
    try {
      write(sanitizer.push(formatSelfReference(cliApp.selfReference)) + sanitizer.flush());
      return 0;
    } finally {
      cliApp.close();
      await cliApp.sandbox.close();
    }
  }
  const areaArg = doctorAreaArg(args);
  if (areaArg !== undefined && !isDoctorArea(areaArg)) {
    write(
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
      write(
        sanitizer.push(`${JSON.stringify(toSafeReport(report), null, 2)}\n`) + sanitizer.flush(),
      );
    } else if (safe) {
      write(sanitizer.push(formatSafeDoctorReport(toSafeReport(report))) + sanitizer.flush());
    } else if (json) {
      write(sanitizer.push(`${JSON.stringify(report, null, 2)}\n`) + sanitizer.flush());
    } else {
      write(sanitizer.push(formatSiralosDoctorReport(report)) + sanitizer.flush());
    }
    return doctorExitCodeFor(report);
  } catch (error: unknown) {
    write(`Siralos doctor failed to run: ${describeError(error)}\n`);
    return 2;
  } finally {
    cliApp.close();
    await cliApp.sandbox.close();
  }
}
