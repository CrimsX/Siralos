import { afterEach, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import {
  computeWorkspaceRevisionHandle,
  createWorkspaceRevisionRegistry,
  type GDScriptStructure,
  type ToolExecutionResult,
} from "@solaris/core";
import {
  createBehaviorLoopHarness,
  FIXTURE_PATH,
  readWorkspaceFile,
  type BehaviorLoopHarness,
} from "./behavior-harness.js";

/**
 * Workspace revision and structural-read behavior fixtures (Stage 3
 * milestone 3), verified at the final observable boundary: the actual
 * read-tool results, the actual mutation path (prepare/apply through the
 * development service), and the actual file contents.
 */

function sha256(text: string): string {
  return createHash("sha256").update(text).digest("hex");
}

const REVISION_PATTERN = /^rev_[0-9a-f]{32}$/;

function readResult(harness: BehaviorLoopHarness, input: unknown): Promise<ToolExecutionResult> {
  return harness.workspaceRead.execute(input, {});
}

interface Fixture {
  readonly harness: BehaviorLoopHarness;
  cleanup(): Promise<void>;
}

async function fixture(options: { readonly qualityStage?: boolean } = {}): Promise<Fixture> {
  const harness = await createBehaviorLoopHarness(options);
  return { harness, cleanup: () => harness.cleanup() };
}

describe("Behavior 1 — an exact read returns an opaque revision backed by the SHA-256", () => {
  it("the handle maps internally to the exact file identity", async () => {
    const f = await fixture();
    try {
      const result = await readResult(f.harness, { path: FIXTURE_PATH, mode: "exact" });
      expect(result.status).toBe("success");
      if (result.status !== "success") {
        return;
      }
      const output = result.output as Record<string, unknown>;
      const revision = output["revision"] as string;
      expect(revision).toMatch(REVISION_PATTERN);
      const identity = f.harness.revisions.resolve(revision);
      expect(identity?.path).toBe(FIXTURE_PATH);
      expect(identity?.sha256).toBe(output["sha256"]);
      // The exact read also records the observation (multi-agent groundwork).
      expect(f.harness.revisions.observedReads().length).toBeGreaterThan(0);
    } finally {
      await f.cleanup();
    }
  });
});

describe("Behavior 2 — revision handles never resolve across workspaces", () => {
  it("the same relative path in a different workspace rejects the handle", async () => {
    const f = await fixture();
    try {
      const result = await readResult(f.harness, { path: FIXTURE_PATH });
      if (result.status !== "success") {
        return;
      }
      const revision = (result.output as Record<string, unknown>)["revision"] as string;
      const other = createWorkspaceRevisionRegistry({ workspaceFingerprint: "other-workspace" });
      expect(other.resolve(revision)).toBeNull();
      expect(computeWorkspaceRevisionHandle("other-workspace", FIXTURE_PATH, sha256("x"))).not.toBe(
        revision,
      );
    } finally {
      await f.cleanup();
    }
  });
});

describe("Behaviors 3-6 — mutation lifecycle and stale rejection", () => {
  async function applyEdit(
    harness: BehaviorLoopHarness,
    expectedRevision: string | undefined,
    expectedSha256: string | undefined,
    replacement = { oldText: "move_and_slide()", newText: "move_and_slide(Vector2.UP)" },
  ): Promise<"applied" | "stale_revision" | "invalid_input" | "failed"> {
    const prepared = await harness.development.prepareChangeSet(
      {
        changes: [
          {
            operation: "edit",
            path: FIXTURE_PATH,
            ...(expectedRevision !== undefined ? { expectedRevision } : {}),
            ...(expectedSha256 !== undefined ? { expectedSha256 } : {}),
            replacements: [replacement],
          },
        ],
      },
      {},
    );
    if (
      prepared.status === "stale_revision" ||
      prepared.status === "invalid_input" ||
      prepared.status === "failed"
    ) {
      return prepared.status;
    }
    if (prepared.status !== "ready") {
      return "failed";
    }
    const applied = await harness.development.applyChangeSet(prepared.changeSetId, {
      approvedDigest: prepared.digest,
    });
    return applied.status === "applied" ? "applied" : "failed";
  }

  it("a successful mutation produces a new revision and rejects the old one (behaviors 3-4, 6)", async () => {
    const f = await fixture({ qualityStage: false });
    try {
      const harness = f.harness;
      await harness.startWorkflow("develop fixture");
      const read = await readResult(harness, { path: FIXTURE_PATH });
      expect(read.status).toBe("success");
      if (read.status !== "success") {
        return;
      }
      const revisionA = (read.output as Record<string, unknown>)["revision"] as string;
      const before = await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH);

      // First edit with a fresh revision succeeds and issues revision B.
      expect(await applyEdit(harness, revisionA, undefined)).toBe("applied");
      const revisionB = harness.revisions.currentRevision(FIXTURE_PATH);
      expect(revisionB).not.toBeNull();
      expect(revisionB).not.toBe(revisionA);
      const afterFirst = await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH);
      expect(afterFirst).toContain("move_and_slide(Vector2.UP)");

      // A second edit using revision A is rejected as stale and changes nothing.
      expect(await applyEdit(harness, revisionA, undefined)).toBe("stale_revision");
      const afterStale = await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH);
      expect(afterStale).toBe(afterFirst);
      void before;

      // A fresh exact read yields B, and a second approved edit with B succeeds.
      const readB = await readResult(harness, { path: FIXTURE_PATH });
      expect(readB.status).toBe("success");
      if (readB.status !== "success") {
        return;
      }
      expect((readB.output as Record<string, unknown>)["revision"]).toBe(revisionB);
      expect(
        await applyEdit(harness, revisionB ?? undefined, undefined, {
          oldText: "move_and_slide(Vector2.UP)",
          newText: "move_and_slide(Vector2.DOWN)",
        }),
      ).toBe("applied");
      expect(harness.revisions.currentRevision(FIXTURE_PATH)).not.toBe(revisionB);
      expect(await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH)).toContain(
        "move_and_slide(Vector2.DOWN)",
      );
    } finally {
      await f.cleanup();
    }
  });

  it("an external modification invalidates the old revision assumption (behavior 5)", async () => {
    const f = await fixture({ qualityStage: false });
    try {
      const harness = f.harness;
      await harness.startWorkflow("develop fixture");
      const read = await readResult(harness, { path: FIXTURE_PATH });
      if (read.status !== "success") {
        return;
      }
      const revisionA = (read.output as Record<string, unknown>)["revision"] as string;
      // External/user modification between read and mutation.
      const { writeFile } = await import("node:fs/promises");
      await writeFile(
        `${harness.workspace.root}/${FIXTURE_PATH}`,
        "extends CharacterBody2D\n\n# externally edited\n",
        "utf8",
      );
      const registrySizeBefore = f.harness.revisions.size;
      const outcome = await applyEdit(harness, revisionA, undefined);
      expect(outcome).toBe("stale_revision");
      // The file is untouched by the rejected mutation.
      expect(await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH)).toContain(
        "# externally edited",
      );
      // A failed preparation has no registry side effect: the externally
      // changed state is not issued a revision by the refusal itself.
      expect(f.harness.revisions.size).toBe(registrySizeBefore);
      const externalContent = "extends CharacterBody2D\n\n# externally edited\n";
      expect(
        f.harness.revisions.revisionForState(FIXTURE_PATH, sha256(externalContent)),
      ).toBeNull();
      expect(f.harness.revisions.currentRevision(FIXTURE_PATH)).toBe(revisionA);
    } finally {
      await f.cleanup();
    }
  });

  it("a change set without any pre-state identity is rejected (summary text cannot substitute)", async () => {
    const f = await fixture({ qualityStage: false });
    try {
      const harness = f.harness;
      await harness.startWorkflow("develop fixture");
      const outcome = await applyEdit(harness, undefined, undefined);
      expect(outcome).toBe("invalid_input");
    } finally {
      await f.cleanup();
    }
  });

  it("a forged revision handle is rejected (behaviors 13-14)", async () => {
    const f = await fixture({ qualityStage: false });
    try {
      const harness = f.harness;
      await harness.startWorkflow("develop fixture");
      expect(await applyEdit(harness, "rev_" + "f".repeat(32), undefined)).toBe("invalid_input");
    } finally {
      await f.cleanup();
    }
  });
});

describe("Behavior 7-11 — structural and summary reads", () => {
  const STRUCTURE_SOURCE = `@tool
extends CharacterBody2D
class_name PlayerController

signal health_changed(old_value: int)
# func fake_function(): comment only
var description = "var not_a_property"

@export var speed: float = 300.0
const MAX_HEALTH: int = 100

func _physics_process(delta: float) -> void:
	pass

func take_damage(amount: int) -> void:
	pass
`;

  async function writeStructure(harness: BehaviorLoopHarness): Promise<void> {
    const { writeFile } = await import("node:fs/promises");
    await writeFile(`${harness.workspace.root}/${FIXTURE_PATH}`, STRUCTURE_SOURCE, "utf8");
  }

  it("structural reads report representative declarations with the revision (behaviors 7, 8, 10)", async () => {
    const f = await fixture();
    try {
      await writeStructure(f.harness);
      const result = await readResult(f.harness, { path: FIXTURE_PATH, mode: "structural" });
      expect(result.status).toBe("success");
      if (result.status !== "success") {
        return;
      }
      const output = result.output as Record<string, unknown>;
      expect(output["mode"]).toBe("structural");
      expect(output["revision"]).toMatch(REVISION_PATTERN);
      const structure = output["structure"] as GDScriptStructure;
      expect(structure.status).toBe("complete");
      expect(structure.extendsType).toBe("CharacterBody2D");
      expect(structure.className).toBe("PlayerController");
      expect(structure.signals.map((signal) => signal.name)).toEqual(["health_changed"]);
      expect(structure.properties.map((property) => property.name)).toEqual([
        "description",
        "speed",
      ]);
      expect(structure.properties[1]?.annotations).toEqual(["export"]);
      expect(structure.constants.map((constant) => constant.name)).toEqual(["MAX_HEALTH"]);
      expect(structure.functions.map((fn) => fn.name)).toEqual(["_physics_process", "take_damage"]);
      // Keywords in comments/strings created no fake declarations.
      expect(structure.functions.some((fn) => fn.name === "fake_function")).toBe(false);
      expect(structure.properties.some((property) => property.name === "not_a_property")).toBe(
        false,
      );
    } finally {
      await f.cleanup();
    }
  });

  it("invalid GDScript yields a partial result with parser errors, never fabricated data (behavior 9)", async () => {
    const f = await fixture();
    try {
      const { writeFile } = await import("node:fs/promises");
      await writeFile(
        `${f.harness.workspace.root}/${FIXTURE_PATH}`,
        'extends Node\nvar x = """\nunterminated\nfunc fake():\n\tpass\n',
        "utf8",
      );
      const result = await readResult(f.harness, { path: FIXTURE_PATH, mode: "structural" });
      expect(result.status).toBe("success");
      if (result.status !== "success") {
        return;
      }
      const structure = (result.output as Record<string, unknown>)[
        "structure"
      ] as GDScriptStructure;
      expect(structure.status).toBe("partial");
      expect(structure.parserErrors.length).toBeGreaterThan(0);
      expect(structure.functions.some((fn) => fn.name === "fake")).toBe(false);
    } finally {
      await f.cleanup();
    }
  });

  it("summary reads are bounded, advisory, and carry the revision (behaviors 11-12)", async () => {
    const f = await fixture();
    try {
      await writeStructure(f.harness);
      const result = await readResult(f.harness, { path: FIXTURE_PATH, mode: "summary" });
      expect(result.status).toBe("success");
      if (result.status !== "success") {
        return;
      }
      const output = result.output as Record<string, unknown>;
      expect(output["mode"]).toBe("summary");
      expect(output["advisory"]).toBe(true);
      const revision = output["revision"] as string;
      expect(revision).toMatch(REVISION_PATTERN);
      expect(output["summary"]).toContain(`@ ${revision}`);
      expect(output["summary"]).toContain("not authoritative source");
      // The summary becomes stale when the file changes: its revision no
      // longer matches the file's current state after a mutation.
      const { writeFile } = await import("node:fs/promises");
      await writeFile(
        `${f.harness.workspace.root}/${FIXTURE_PATH}`,
        STRUCTURE_SOURCE + "\n# changed\n",
        "utf8",
      );
      const currentSha = sha256(STRUCTURE_SOURCE + "\n# changed\n");
      expect(f.harness.revisions.resolve(revision)?.sha256).not.toBe(currentSha);
    } finally {
      await f.cleanup();
    }
  });

  it("structural/summary modes return an explicit unsupported result for non-GDScript files", async () => {
    const f = await fixture();
    try {
      const { writeFile } = await import("node:fs/promises");
      await writeFile(
        `${f.harness.workspace.root}/README.md`,
        "# Fixture\nPlain markdown, not GDScript.\n",
        "utf8",
      );
      const result = await readResult(f.harness, { path: "README.md", mode: "summary" });
      expect(result.status).toBe("success");
      if (result.status !== "success") {
        return;
      }
      const output = result.output as Record<string, unknown>;
      expect(output["supported"]).toBe(false);
    } finally {
      await f.cleanup();
    }
  });
});

describe("Behavior 15 — multi-file changesets reject a stale member revision", () => {
  it("one stale file fails the whole prepared change set", async () => {
    const f = await fixture({ qualityStage: false });
    try {
      const harness = f.harness;
      await harness.startWorkflow("develop fixture");
      const { writeFile } = await import("node:fs/promises");
      await writeFile(
        `${harness.workspace.root}/second.gd`,
        "extends Node\nfunc second():\n\tpass\n",
        "utf8",
      );
      const readSecond = await readResult(harness, { path: "second.gd" });
      const readPrimary = await readResult(harness, { path: FIXTURE_PATH });
      if (readSecond.status !== "success" || readPrimary.status !== "success") {
        return;
      }
      const secondRevision = (readSecond.output as Record<string, unknown>)["revision"] as string;
      const primaryRevision = (readPrimary.output as Record<string, unknown>)["revision"] as string;
      // Stale the primary file externally.
      await writeFile(
        `${harness.workspace.root}/${FIXTURE_PATH}`,
        "extends CharacterBody2D\n# externally edited\n",
        "utf8",
      );
      const prepared = await harness.development.prepareChangeSet(
        {
          changes: [
            {
              operation: "edit",
              path: FIXTURE_PATH,
              expectedRevision: primaryRevision,
              replacements: [
                { oldText: "move_and_slide()", newText: "move_and_slide(Vector2.UP)" },
              ],
            },
            {
              operation: "edit",
              path: "second.gd",
              expectedRevision: secondRevision,
              replacements: [{ oldText: "pass", newText: "return 1" }],
            },
          ],
        },
        {},
      );
      // The stale member fails the transaction before anything applies.
      expect(prepared.status).toBe("stale_revision");
      expect(await readWorkspaceFile(harness.workspace.root, "second.gd")).toContain("pass");
    } finally {
      await f.cleanup();
    }
  });
});

describe("Behaviors 16-18 — /develop uses revision-aware reads and repairs", () => {
  let f: Fixture;
  afterEach(async () => {
    await f.cleanup();
  });

  it("the first mutation succeeds with a fresh revision and validation is bound to it", async () => {
    f = await fixture();
    const harness = f.harness;
    await harness.startWorkflow("develop fixture");
    await harness.runPrompt("develop fixture");
    const task = await harness.finalizeTask();
    expect(task?.phase).toBe("completed");
    const revision = harness.revisions.currentRevision(FIXTURE_PATH);
    expect(revision).toMatch(REVISION_PATTERN);
    // The mutation evidence carries the post-edit revision.
    const mutationEvidence = task?.evidence.find((entry) => entry.kind === "mutation_receipt");
    expect(
      mutationEvidence?.source.type === "mutation" ? mutationEvidence.source.revision : null,
    ).toBe(revision);
  });

  it("the repair cycle operates against the new post-edit revision", async () => {
    f = await fixture();
    const harness = f.harness;
    // A pre-loop exact read establishes the initial revision.
    const initial = await readResult(harness, { path: FIXTURE_PATH });
    expect(initial.status).toBe("success");
    if (initial.status !== "success") {
      return;
    }
    const initialRevision = (initial.output as Record<string, unknown>)["revision"] as string;
    // The first post-edit parse fails; the repair fixes it (as in the
    // existing repair-loop fixture).
    harness.parserControl.queuedResults.push({
      path: FIXTURE_PATH,
      valid: false,
      diagnostics: [
        {
          source: "godot-check-only",
          severity: "error",
          path: FIXTURE_PATH,
          line: 4,
          column: 3,
          code: null,
          message: "Parse error: Unexpected token )",
          rawCategory: "SCRIPT ERROR",
        },
      ],
    });
    await harness.startWorkflow("develop fixture with repair");
    await harness.runPrompt("develop fixture with repair");
    const task = await harness.finalizeTask();
    expect(task?.phase).toBe("completed");
    const finalRevision = harness.revisions.currentRevision(FIXTURE_PATH);
    expect(finalRevision).toMatch(REVISION_PATTERN);
    // The loop issued new post-edit revisions distinct from the initial state.
    expect(finalRevision).not.toBe(initialRevision);
    // The final state is the repaired one.
    expect(await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH)).toContain(
      "move_and_slide(Vector2.UP)",
    );
  });
});

describe("Behaviors 19-21 — containment, protected paths, and handle authority", () => {
  it("all read modes preserve workspace containment", async () => {
    const f = await fixture();
    try {
      for (const mode of ["exact", "structural", "summary"] as const) {
        const result = await readResult(f.harness, { path: "../outside.gd", mode });
        expect(result.status).toBe("denied");
      }
    } finally {
      await f.cleanup();
    }
  });

  it("protected paths stay inaccessible through structural/summary modes", async () => {
    const f = await fixture();
    try {
      const { mkdir, writeFile } = await import("node:fs/promises");
      await mkdir(`${f.harness.workspace.root}/.git`, { recursive: true });
      await writeFile(
        `${f.harness.workspace.root}/.git/config`,
        "[core]\n\trepositoryformatversion = 0\n",
        "utf8",
      );
      for (const mode of ["structural", "summary"] as const) {
        const result = await readResult(f.harness, { path: ".git/config", mode });
        expect(result.status).toBe("denied");
      }
    } finally {
      await f.cleanup();
    }
  });

  it("a revision handle never grants path or permission access", async () => {
    const f = await fixture();
    try {
      // Issue a handle for a file in the excluded directory directly.
      const forged = f.harness.revisions.issue(".git/config", "a".repeat(64));
      expect(forged).toMatch(REVISION_PATTERN);
      // The handle exists, but the read for that path is still denied.
      const result = await readResult(f.harness, { path: ".git/config", mode: "exact" });
      expect(result.status).toBe("denied");
    } finally {
      await f.cleanup();
    }
  });
});

describe("Behaviors 22-23 — Stage 2 develop regressions through revision-aware reads", () => {
  let f: Fixture;
  afterEach(async () => {
    await f.cleanup();
  });

  it("clean success still completes", async () => {
    f = await fixture();
    await f.harness.startWorkflow("develop fixture");
    await f.harness.runPrompt("develop fixture");
    expect((await f.harness.finalizeTask())?.phase).toBe("completed");
  });

  it("failed validation still blocks completion", async () => {
    f = await fixture();
    f.harness.parserControl.resultsByPath.set(FIXTURE_PATH, {
      valid: false,
      diagnostics: [
        {
          source: "godot-check-only",
          severity: "error",
          path: FIXTURE_PATH,
          line: 4,
          column: 3,
          code: null,
          message: "Parse error: Unexpected token )",
          rawCategory: "SCRIPT ERROR",
        },
      ],
    });
    await f.harness.startWorkflow("develop fixture");
    await f.harness.runPrompt("develop fixture");
    const task = await f.harness.finalizeTask();
    expect(task?.phase).toBe("failed");
    expect(task?.phase).not.toBe("completed");
  });
});

describe("Effect tests — final boundaries", () => {
  it("a stale-revision mutation never alters the file (effect)", async () => {
    const f = await fixture({ qualityStage: false });
    try {
      const harness = f.harness;
      await harness.startWorkflow("develop fixture");
      const read = await readResult(harness, { path: FIXTURE_PATH });
      if (read.status !== "success") {
        return;
      }
      const revisionA = (read.output as Record<string, unknown>)["revision"] as string;
      // The file changes externally after the read.
      const { writeFile } = await import("node:fs/promises");
      await writeFile(
        `${harness.workspace.root}/${FIXTURE_PATH}`,
        "extends CharacterBody2D\n# externally edited\n",
        "utf8",
      );
      const before = await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH);
      const prepared = await harness.development.prepareChangeSet(
        {
          changes: [
            {
              operation: "edit",
              path: FIXTURE_PATH,
              expectedRevision: revisionA,
              replacements: [
                { oldText: "move_and_slide()", newText: "move_and_slide(Vector2.UP)" },
              ],
            },
          ],
        },
        {},
      );
      expect(prepared.status).toBe("stale_revision");
      // (The workflow itself already mutated nothing; no checkpoint was created.)
      expect(await harness.store.list()).toHaveLength(0);
      expect(await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH)).toBe(before);
    } finally {
      await f.cleanup();
    }
  });

  it("the model-facing read result exposes only the opaque revision, never internal identity", async () => {
    const f = await fixture();
    try {
      const result = await readResult(f.harness, { path: FIXTURE_PATH, mode: "summary" });
      expect(result.status).toBe("success");
      if (result.status !== "success") {
        return;
      }
      const serialized = JSON.stringify(result.output);
      expect(serialized).toMatch(/rev_[0-9a-f]{32}/);
      // The workspace fingerprint (canonical JSON of the root) never leaks.
      expect(serialized).not.toContain("workspaceRoot");
    } finally {
      await f.cleanup();
    }
  });
});
