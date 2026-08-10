import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createCliApplication } from "./bootstrap/create-application.js";
import { runInteractiveSession, type SessionIO, type SessionInfo } from "./interactive-session.js";

const tempDirectories: string[] = [];

async function makeTempDirectory(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "solaris-session-test-"));
  tempDirectories.push(directory);
  return directory;
}

async function withConfigFile(content: unknown): Promise<string> {
  const directory = await makeTempDirectory();
  const path = join(directory, "config.json");
  await writeFile(path, JSON.stringify(content));
  return path;
}

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

class ScriptedIO implements SessionIO {
  private readonly lines: readonly string[];
  private index = 0;
  private readonly chunks: string[] = [];

  constructor(lines: readonly string[]) {
    this.lines = lines;
  }

  ask(_prompt: string): Promise<string | null> {
    if (this.index >= this.lines.length) {
      return Promise.resolve(null);
    }
    const line = this.lines[this.index];
    this.index += 1;
    return Promise.resolve(line === undefined ? null : line);
  }

  write(text: string): void {
    this.chunks.push(text);
  }

  clear(): void {
    this.chunks.push("[clear]");
  }

  get text(): string {
    return this.chunks.join("");
  }
}

async function createComposedSession(lines: readonly string[], configPath: string) {
  const io = new ScriptedIO(lines);
  const app = await createCliApplication({ configPath });
  const sessionInfo: SessionInfo = {
    workspaceRoot: app.workspaceRoot,
    configPath: app.configPath,
    policy: app.policy,
    profile: app.profile,
    provider: app.provider,
    selfReference: app.selfReference,
    tasks: app.tasks,
    taskSources: app.taskSources,
    projection: app.projection,
    revisions: app.revisions,
    workspaceRead: app.workspaceRead,
    instructions: app.instructions,
    projectKnowledge: app.projectKnowledge,
    references: app.references,
    referenceMaterializer: app.referenceMaterializer,
    referenceConfigError: app.referenceConfigError,
    research: app.research,
    researchSources: app.researchSources,
    planner: app.planner,
    tools: app.tools,
    security: app.security,
    git: app.git,
    godot: app.godot,
    godotProbe: app.godotProbe,
    knowledge: app.knowledge,
    diagnostics: app.diagnostics,
    language: app.language,
    development: app.development,
    reviewer: {
      review(): Promise<{ type: "deny"; reason: string }> {
        return Promise.resolve({ type: "deny", reason: "not configured" });
      },
    },
    checkpoints: app.checkpoints,
    undo: app.undo,
    runners: app.runners,
    sandbox: app.sandbox,
  };
  return { io, app, sessionInfo };
}

describe("reference and research session commands", () => {
  it("renders /references, /reference, /research-status, and the /status research line", async () => {
    const directory = await makeTempDirectory();
    const reference = join(directory, "engine-docs");
    await mkdir(reference);
    await writeFile(join(reference, "README.md"), "# Engine docs\n");
    const configPath = await withConfigFile({
      sandbox: { profile: "inspect" },
      references: {
        "engine-docs": {
          kind: "local-directory",
          path: reference,
          description: "Engine documentation",
        },
      },
    });
    const { io, app, sessionInfo } = await createComposedSession(
      ["/references", "/reference engine-docs", "/reference nope", "/research-status", "/status"],
      configPath,
    );
    try {
      await runInteractiveSession(io, app.application, sessionInfo);
      const text = io.text;

      // /references
      expect(text).toContain("References (1)");
      expect(text).toContain("@reference/engine-docs");
      expect(text).toContain("local-directory");
      expect(text).toContain("fingerprint");
      expect(text).toContain("not-required");
      expect(text).toContain("explicit-user");
      expect(text).toContain("ready");
      // The configured path is shown as the user typed it (never a managed
      // cache path).
      expect(text).toContain(reference);

      // /reference <alias>
      expect(text).toContain("Reference: @reference/engine-docs");
      expect(text).toContain("Description: Engine documentation");
      expect(text).toContain("Availability: ready");
      expect(text).toContain("Resolved revision: fingerprint");

      // /reference <unknown>
      expect(text).toContain("Unknown reference: nope");

      // /research-status
      expect(text).toContain("Research: disabled");
      expect(text).toContain(
        "Sources (2): repository (GitHub repository research), godot-docs (Godot official documentation)",
      );
      expect(text).toContain("Active requests: 0");
      expect(text).toContain("Recent evidence: 0");

      // /status research line
      expect(text).toContain("Research: disabled (2 sources)");
    } finally {
      app.close();
    }
  });

  it("renders a references configuration error at the top of /references without crashing", async () => {
    const configPath = await withConfigFile({
      references: {
        bad: { kind: "local-directory", path: "relative/path" },
      },
    });
    const { io, app, sessionInfo } = await createComposedSession(["/references"], configPath);
    try {
      expect(app.referenceConfigError).toContain("not absolute");
      await runInteractiveSession(io, app.application, sessionInfo);
      expect(io.text).toContain("References configuration error");
      expect(io.text).toContain("No references are available.");
    } finally {
      app.close();
    }
  });

  it("renders an empty /references when nothing is configured", async () => {
    const configPath = await withConfigFile({ sandbox: { profile: "inspect" } });
    const { io, app, sessionInfo } = await createComposedSession(["/references"], configPath);
    try {
      await runInteractiveSession(io, app.application, sessionInfo);
      expect(io.text).toContain("No references are configured.");
    } finally {
      app.close();
    }
  });
});
