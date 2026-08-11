import { createAcceptanceEvaluator } from "@solaris/core";
import { formatExecutorBrief, formatMilestoneManifest } from "../output.js";
import type { SessionIO, SessionInfo } from "./session-types.js";

/**
 * Executor briefing inspection surface (executor briefing foundation):
 *
 *   /brief      — compile and show the current task's bounded executor brief
 *   /milestone  — show the current milestone manifest and its evidence-backed
 *                 acceptance status
 *
 * Both are read-only and provider-free (dry-run briefing, §29). Briefing
 * semantics live in core; these handlers only render host-compiled
 * artifacts.
 */

export function runBriefCommand(io: SessionIO, sessionInfo: SessionInfo): void {
  const brief = sessionInfo.briefing.latestOrCompile();
  if (brief === null) {
    io.write("No task is tracked yet. Start one with /task <request> or /develop <request>.\n");
    return;
  }
  io.write(formatExecutorBrief(brief, sessionInfo.briefing.fingerprint()));
}

export function runMilestoneCommand(io: SessionIO, sessionInfo: SessionInfo): void {
  const handle = sessionInfo.tasks.latestTask();
  const report =
    handle === null
      ? null
      : createAcceptanceEvaluator().evaluate({
          manifest: sessionInfo.milestoneManifest,
          evidence: handle.snapshot().evidence,
          acceptance: handle.snapshot().acceptance,
        });
  io.write(formatMilestoneManifest(sessionInfo.milestoneManifest, report));
}
