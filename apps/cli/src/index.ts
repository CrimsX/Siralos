#!/usr/bin/env node

import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import { createCliApplication } from "./bootstrap/create-application.js";
import { runInteractiveSession, type SessionIO } from "./interactive-session.js";
import { describeError, formatHeader } from "./output.js";

async function main(): Promise<number> {
  const { application, providerId, workspaceRoot, tools } = await createCliApplication();
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
  stdout.write(formatHeader(providerId));
  const exitCode = await runInteractiveSession(io, application, { workspaceRoot, tools });
  readline.close();
  stdin.destroy();
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
