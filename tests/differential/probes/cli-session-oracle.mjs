/**
 * cli-session oracle probe (differential harness, Stage 3R R13.5a).
 *
 * Drives the REAL TypeScript interactive session composition
 * (`runInteractiveSession`) with scripted input-queue lines, a fake
 * SessionIO capture, and the real CLI application built by the bootstrap
 * factory over the deterministic fake provider inside a throwaway
 * workspace. Emits one bounded case record per scenario group.
 *
 * Deterministic: scripted queue (no TTY), injected fixture config, no
 * network (research is denied by every built-in profile), and the
 * sanitizer statefulness is exercised exactly as at the real boundary.
 */
const input = JSON.parse(readFileSync(0, "utf8"));

import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// The composition canonicalizes the process cwd into the workspace root;
// isolate every filesystem effect inside a throwaway directory BEFORE the
// real modules are imported.
const workspace = mkdtempSync(join(tmpdir(), "siralos-clisession-"));
writeFileSync(
  join(workspace, "config.json"),
  JSON.stringify({ sandbox: { profile: "inspect" } }),
  "utf8",
);
process.chdir(workspace);

// The injected clock lives inside the composition defaults; the probe

async function main() {
  const { createCliApplication } =
    await import("../../../apps/cli/src/bootstrap/create-application.ts");
  const { runInteractiveSession } = await import("../../../apps/cli/src/interactive-session.ts");
  const { TerminalSanitizer } = await import("../../../apps/cli/src/output/sanitize.ts");

  const { COMMAND_CATALOG_IDS } = await import("@siralos/core");
  let workspaceRoot = "";
  let configPath = "";

  const cliApp = await createCliApplication({
    configPath: join(workspace, "config.json"),
  });

  function makeIo(capture) {
    return {
      async ask(prompt) {
        capture.prompts.push(prompt);
        return null;
      },
      write(text) {
        // The production boundary wraps every session write in the
        // terminal sanitizer (push+flush); capture post-boundary bytes.
        const rendered = capture.sanitizer.push(text) + capture.sanitizer.flush();
        // The reference prints its absolute workspace/config roots here;
        // those are host-machine facts, not behavior. Canonicalize them so
        // records stay byte-stable across machines (mirrors the
        // workspace-relative discipline of the R4 probes).
        capture.writes.push(
          rendered.split(workspaceRoot).join("<workspace>").split(configPath).join("<config>"),
        );
      },
      clear() {
        capture.cleared += 1;
      },
    };
  }

  function makeQueue(lines) {
    let index = 0;
    return {
      async ask(_prompt) {
        if (index >= lines.length) {
          return { kind: "eof" };
        }
        const value = lines[index];
        index += 1;
        return { kind: "answer", value };
      },
    };
  }

  function sessionInfoFor() {
    // Mirror index.ts destructuring exactly (composition-owned wiring).
    const s = cliApp;
    return {
      workspaceRoot: s.workspaceRoot,
      configPath: s.configPath,
      policy: s.policy,
      profile: s.profile,
      provider: s.provider,
      selfReference: s.selfReference,
      tools: s.tools,
      security: s.security,
      git: s.git,
      godot: s.godot,
      godotProbe: s.godotProbe,
      knowledge: s.knowledge,
      diagnostics: s.diagnostics,
      language: s.language,
      development: s.development,
      reviewer: {
        async review() {
          return { type: "deny", reason: "probe" };
        },
      },
      checkpoints: s.checkpoints,
      undo: s.undo,
      runners: s.runners,
      sandbox: s.sandbox,
      tasks: s.tasks,
      taskSources: s.taskSources,
      projection: s.projection,
      revisions: s.revisions,
      workspaceRead: s.workspaceRead,
      instructions: s.instructions,
      projectKnowledge: s.projectKnowledge,
      references: s.references,
      referenceMaterializer: s.referenceMaterializer,
      referenceConfigError: s.referenceConfigError,
      research: s.research,
      researchSources: s.researchSources,
      planner: s.planner,
      briefing: s.briefing,
      milestoneManifest: s.milestoneManifest,
    };
  }

  async function runSession(lines) {
    workspaceRoot = cliApp.workspaceRoot;
    configPath = cliApp.configPath;
    const capture = {
      writes: [],
      prompts: [],
      cleared: 0,
      sanitizer: new TerminalSanitizer(),
    };
    const exitCode = await runInteractiveSession(
      makeIo(capture),
      cliApp.application,
      sessionInfoFor(),
      undefined,
      makeQueue(lines),
    );
    return { exitCode, ...capture };
  }

  const cases = [];
  for (const inputCase of input.cases) {
    switch (inputCase.name) {
      case "input-parsing": {
        const catalogParse = [];
        for (const id of COMMAND_CATALOG_IDS) {
          const session = await runSession([`/${id}`]);
          const joined = session.writes.join("");
          catalogParse.push({
            command: id,
            unknownCommandRendered: joined.includes("Unknown command"),
          });
        }
        const whitespace = await runSession(["   ", ""]);
        const unknown = await runSession(["/definitely-not-a-command"]);
        const plainPrompt = await runSession(["hello there"]);
        cases.push({
          name: "input-parsing",
          catalogParse,
          emptyWrites: whitespace.writes.length,
          emptyExitCode: whitespace.exitCode,
          unknownWrites: unknown.writes,
          promptWrites: plainPrompt.writes,
          promptExitCode: plainPrompt.exitCode,
        });
        break;
      }
      case "session-lifecycle": {
        const drained = await runSession(["/help", "/commands", "/clear", "/status"]);
        cases.push({
          name: "session-lifecycle",
          exitCode: drained.exitCode,
          cleared: drained.cleared,
          promptCount: drained.prompts.length,
          writeCount: drained.writes.length,
        });
        break;
      }
      case "help-and-commands": {
        const help = await runSession(["/help"]);
        const commands = await runSession(["/commands"]);
        cases.push({
          name: "help-and-commands",
          helpWrites: help.writes,
          commandsWrites: commands.writes,
        });
        break;
      }
      case "status-view": {
        const status = await runSession(["/status"]);
        cases.push({ name: "status-view", writes: status.writes });
        break;
      }
      case "unknown-command": {
        const unknown = await runSession(["/nope arg"]);
        cases.push({ name: "unknown-command", writes: unknown.writes });
        break;
      }
      case "prompt-turn": {
        const turn = await runSession(["add a greeting to src/app.ts", "/status"]);
        cases.push({ name: "prompt-turn", writes: turn.writes });
        break;
      }
      default:
        throw new Error(`unknown cli-session fixture case ${inputCase.name}`);
    }
  }

  process.stdout.write(JSON.stringify({ cases }));
}

main()
  .then(() => {
    try {
      rmSync(workspace, { recursive: true, force: true });
    } catch {
      // Windows may hold the checkpoint-store handle briefly; the
      // throwaway directory is cleaned by the OS anyway.
    }
  })
  .catch((error) => {
    try {
      rmSync(workspace, { recursive: true, force: true });
    } catch {
      // best-effort cleanup
    }
    console.error(String(error instanceof Error ? (error.stack ?? error.message) : error));
    process.exit(2);
  });
