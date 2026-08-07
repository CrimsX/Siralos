#!/usr/bin/env node

import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import { createInteractiveApprovalReviewer } from "./approval/approval-reviewer.js";
import { createCliApplication } from "./bootstrap/create-application.js";
import { runSandboxDoctor } from "./bootstrap/sandbox-doctor.js";
import { runInteractiveSession, type SessionIO, type SessionInfo } from "./interactive-session.js";
import { describeError, formatDoctor, formatHeader } from "./output.js";

async function main(): Promise<number> {
  const args = process.argv.slice(2);
  if (args.includes("--sandbox-doctor")) {
    const report = await runSandboxDoctor({ includeProbes: args.includes("--run-probes") });
    stdout.write(formatDoctor(report));
    return 0;
  }
  const readline = createInterface({ input: stdin, output: stdout });
  readline.on("SIGINT", () => {
    readline.close();
  });
  const lines = readline[Symbol.asyncIterator]();
  const io: SessionIO = {
    async ask(prompt: string): Promise<string | null> {
      stdout.write(prompt);
      const result = await lines.next();
      return result.done ? null : result.value;
    },
    write(text: string): void {
      stdout.write(text);
    },
    clear(): void {
      stdout.write("\x1b[2J\x1b[H");
    },
  };
  const reviewer = createInteractiveApprovalReviewer(io);
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
  } = await createCliApplication({ reviewer });
  stdout.write(formatHeader(providerId));
  const sessionInfo: SessionInfo = { workspaceRoot, tools, security, git, checkpoints, undo };
  const exitCode = await runInteractiveSession(io, application, sessionInfo);
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
