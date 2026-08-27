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

  async function runSession(lines) {
    // Fresh application per session to keep determinism (mirrors Rust harness).
    const freshCliApp = await createCliApplication({
      configPath: join(workspace, "config.json"),
    });
    workspaceRoot = freshCliApp.workspaceRoot;
    configPath = freshCliApp.configPath;
    const capture = {
      writes: [],
      prompts: [],
      cleared: 0,
      sanitizer: new TerminalSanitizer(),
    };
    function freshSessionInfoFor() {
      const s = freshCliApp;
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
    const exitCode = await runInteractiveSession(
      makeIo(capture),
      freshCliApp.application,
      freshSessionInfoFor(),
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
      case "godot-commands-unavailable": {
        const session = await runSession([
          "/godot",
          "/godot-installations",
          "/godot-project",
          "/godot-doctor",
          "/godot-probe",
          "/godot-probe-status",
          "/godot-knowledge",
          "/godot-knowledge-refresh",
          "/godot-api test",
        ]);
        cases.push({
          name: "godot-commands-unavailable",
          writes: session.writes,
          writeCount: session.writes.length,
          exitCode: session.exitCode,
        });
        break;
      }
      case "gdscript-commands-unavailable": {
        const session = await runSession([
          "/gdscript-check src/app.gd",
          "/gdscript-diagnostics",
          "/gdscript-lsp",
          "/gdscript-lsp-stop",
          "/gdscript-hover src/app.gd 1 1",
          "/gdscript-complete src/app.gd 1 1",
          "/gdscript-definition src/app.gd 1 1",
        ]);
        cases.push({
          name: "gdscript-commands-unavailable",
          writes: session.writes,
          writeCount: session.writes.length,
          exitCode: session.exitCode,
        });
        break;
      }
      case "develop-commands-unavailable": {
        const session = await runSession([
          "/develop add feature",
          "/plan test",
          "/development-status",
          "/quality",
          "/review-change",
        ]);
        cases.push({
          name: "develop-commands-unavailable",
          writes: session.writes,
          writeCount: session.writes.length,
          exitCode: session.exitCode,
        });
        break;
      }
      case "system-commands-unavailable": {
        const session = await runSession([
          "/sandbox",
          "/permissions",
          "/git-status",
          "/diff",
          "/checkpoints",
          "/undo",
          "/commands",
          "/cancel",
        ]);
        cases.push({
          name: "system-commands-unavailable",
          writes: session.writes,
          writeCount: session.writes.length,
          exitCode: session.exitCode,
        });
        break;
      }
      case "input-queue-ownership": {
        const drained = await runSession([]);
        const clearOnce = await runSession(["/clear"]);
        const interleaved = await runSession(["hello", "/status", "world", "/help"]);
        const empty = await runSession(["   ", "", "   "]);
        cases.push({
          name: "input-queue-ownership",
          drainedExitCode: drained.exitCode,
          drainedWrites: drained.writes.length,
          clearOnceCleared: clearOnce.cleared,
          clearOnceWrites: clearOnce.writes.length,
          interleavedWrites: interleaved.writes,
          interleavedWriteCount: interleaved.writes.length,
          interleavedCleared: interleaved.cleared,
          emptyWrites: empty.writes.length,
          emptyExitCode: empty.exitCode,
        });
        break;
      }
      case "sanitizer-boundary": {
        const sanitizer = new TerminalSanitizer();
        const a = sanitizer.push("\u001b[31mred\u001b[0m");
        const b = sanitizer.push("a\u001b]8;;https://example.com\u0007b");
        const c = sanitizer.push("\u0000\u0001\u0008\u007f\u0080\u009f");
        const d = sanitizer.push("\u001b[");
        const e = sanitizer.push("31mhello");
        const f = sanitizer.flush();
        const g = sanitizer.push("normal\u001b");
        const h = sanitizer.flush();
        const s4 = new TerminalSanitizer();
        const part1 = s4.push("\u{1f600}");
        const part2 = s4.push("x");
        const part3 = s4.flush();
        const session = await runSession(["/status"]);
        const containsWorkspacePlaceholder = session.writes.join("").includes("<workspace>");
        cases.push({
          name: "sanitizer-boundary",
          csiStripped: a,
          oscStripped: b,
          controls: c,
          splitCsi: d + e + f,
          loneEscape: g + h,
          emojiPreserved: part1 + part2 + part3,
          containsWorkspacePlaceholder,
        });
        break;
      }
      case "session-ordering-determinism": {
        const lines = ["/status", "hello world", "/status", "/help"];
        const first = await runSession(lines);
        const second = await runSession(lines);
        const firstJoined = first.writes.join("");
        cases.push({
          name: "session-ordering-determinism",
          firstWrites: first.writes,
          secondWrites: second.writes,
          identical: JSON.stringify(first.writes) === JSON.stringify(second.writes),
          writeCount: first.writes.length,
          containsWorkspacePlaceholder: firstJoined.includes("<workspace>"),
          containsConfigPlaceholder: firstJoined.includes("<config>"),
          exitCode: first.exitCode,
        });
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
