import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { DevelopmentEvent, TaskState } from "@solaris/core";
import {
  createBehaviorLoopHarness,
  readWorkspaceFile,
  sha256Of,
  type BehaviorLoopHarness,
} from "./behavior-harness.js";

/**
 * Stage 3 milestone 11 behavior fixtures (ADR 0027): the unified
 * Godot-native development workflow at the final observable boundary —
 * the unified service, the real checkpoint store, the real temp
 * workspace, and the host task gate.
 */

const SCENE_FIXTURE = `[gd_scene load_steps=3 format=3 uid="uid://player1"]

[ext_resource type="Script" path="res://scripts/player/player.gd" id="1_p"]

[node name="Player" type="CharacterBody2D"]
script = ExtResource("1_p")

[node name="Sprite" type="Sprite2D" parent="Player"]
visible = true
`;

const SCENE_MISSING_SCRIPT = `[gd_scene load_steps=2 format=3 uid="uid://player2"]

[ext_resource type="Script" path="res://scripts/missing.gd" id="1_m"]

[node name="Player" type="CharacterBody2D"]
script = ExtResource("1_m")
`;

async function writeSceneFixture(harness: BehaviorLoopHarness, content: string): Promise<void> {
  await mkdir(join(harness.workspace.root, "scenes"), { recursive: true });
  await writeFile(join(harness.workspace.root, "scenes/player.tscn"), content, "utf8");
}

function mixedTargets(scriptSha256: string): {
  readonly request: string;
  readonly targets: readonly {
    kind: "text" | "scene";
    changes?: unknown;
    path?: string;
    operations?: readonly {
      op: "set_property";
      nodePath: string;
      property: string;
      value: { kind: "float"; value: number };
    }[];
  }[];
} {
  return {
    request: "Add a sprint property and configure the scene value",
    targets: [
      {
        kind: "text",
        changes: {
          changes: [
            {
              operation: "edit",
              path: "scripts/player/player.gd",
              expectedSha256: scriptSha256,
              replacements: [
                { oldText: "move_and_slide()", newText: "move_and_slide(Vector2.UP)" },
              ],
            },
          ],
        },
      },
      {
        kind: "scene",
        path: "scenes/player.tscn",
        operations: [
          {
            op: "set_property",
            nodePath: "Player",
            property: "sprint_speed",
            value: { kind: "float", value: 140.5 },
          },
        ],
      },
    ],
  };
}

function cleanReviewEvents(): readonly DevelopmentEvent[] {
  return [
    { type: "quality_started", developmentId: "unified" },
    { type: "review_started", developmentId: "unified" },
    { type: "review_completed", developmentId: "unified", critical: 0, high: 0, medium: 0, low: 0 },
    { type: "quality_completed", developmentId: "unified", status: "passed" },
  ];
}

describe("S3M11 effect — end-to-end mixed script/native task", () => {
  let harness: BehaviorLoopHarness;
  afterEach(async () => {
    await harness.cleanup();
  });

  it("runs prepare -> approve -> checkpoint -> mutate -> verify -> impact -> validate -> review -> acceptance -> complete", async () => {
    harness = await createBehaviorLoopHarness({
      intelligence: true,
      mutation: true,
      unified: true,
      projection: true,
    });
    await writeSceneFixture(harness, SCENE_FIXTURE);
    const scriptSha256 = sha256Of(
      "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tmove_and_slide()\n",
    );

    const task = harness.startUnifiedTask(
      "Add a sprint property and configure the scene value",
      "mixed",
    );
    // The host-derived mixed surface carries native acceptance criteria.
    expect(task.acceptance.some((criterion) => criterion.criterionId === "native-verified")).toBe(
      true,
    );
    expect(
      task.acceptance.some((criterion) => criterion.criterionId === "cross-surface-consistent"),
    ).toBe(true);

    // Prepare the bounded mixed change set (read-only; nothing written).
    const prepared = await harness.unified!.prepareUnified(mixedTargets(scriptSha256));
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect(prepared.changeSet.surface).toBe("mixed");
    expect(prepared.preview.files).toHaveLength(2);

    // Approve + apply: one checkpoint batch, per-surface verification,
    // cross-surface consistency, and impact derivation.
    const applied = await harness.unified!.applyUnified({
      changeSetId: prepared.changeSet.id,
      approvedDigest: prepared.changeSet.combinedDigest,
    });
    expect(applied.status).toBe("applied");
    if (applied.status !== "applied") {
      return;
    }
    expect(applied.nativeVerification).toEqual([
      { path: "scenes/player.tscn", status: "verified", detail: null },
    ]);
    expect(applied.consistency.consistent).toBe(true);
    expect(applied.parser).toEqual({ checkedFiles: 1, validFiles: 1 });
    expect(applied.lsp).toEqual({ errors: 0, warnings: 0 });
    expect(applied.impact).not.toBeNull();
    // The checkpoint batch covered every affected file before mutation.
    expect(applied.checkpointIds.length).toBeGreaterThanOrEqual(2);
    expect((await harness.store.list()).length).toBe(applied.checkpointIds.length);

    // Independent review (read-only, clean) then host acceptance.
    for (const event of cleanReviewEvents()) {
      harness.unifiedEmit(event);
    }
    const finalTask = harness.finishUnifiedTask("completed");
    expect(finalTask).not.toBeNull();
    if (finalTask === null) {
      return;
    }
    expect(finalTask.phase, `missing: ${completionMissing(harness, finalTask)}`).toBe("completed");
    // Evidence from both surfaces is host-attached.
    expect(finalTask.evidence.some((entry) => entry.kind === "mutation_receipt")).toBe(true);
    expect(finalTask.evidence.some((entry) => entry.kind === "parser_result")).toBe(true);
    expect(finalTask.evidence.some((entry) => entry.kind === "lsp_result")).toBe(true);
    expect(finalTask.evidence.some((entry) => entry.kind === "review_result")).toBe(true);
    expect(
      finalTask.evidence.some(
        (entry) =>
          entry.kind === "validation_result" &&
          entry.source.type === "native_verification" &&
          entry.source.status === "verified",
      ),
    ).toBe(true);
    expect(
      finalTask.evidence.some(
        (entry) => entry.kind === "validation_result" && entry.source.type === "consistency",
      ),
    ).toBe(true);
    expect(
      finalTask.evidence.some(
        (entry) => entry.kind === "validation_result" && entry.source.type === "impact",
      ),
    ).toBe(true);

    // Both surfaces changed exactly as prepared; no raw-text bypass exists.
    const script = await readWorkspaceFile(harness.workspace.root, "scripts/player/player.gd");
    expect(script).toContain("move_and_slide(Vector2.UP)");
    const scene = await readWorkspaceFile(harness.workspace.root, "scenes/player.tscn");
    expect(scene).toContain("sprint_speed = 140.5");
    const toolNames = harness.tools().map((tool) => tool.definition.name);
    expect(toolNames).toContain("godot.prepare_scene_change");
    expect(toolNames).not.toContain("workspace.edit_file");
    // Static workflow: no process-execution tool is registered and no
    // Godot runtime was launched (the language/parser surfaces are fakes).
    expect(toolNames.some((name) => name.startsWith("process."))).toBe(false);
    expect(harness.languageControl.log.some((entry) => entry.includes("spawn"))).toBe(false);
  });
});

describe("S3M11 effect — stale second target blocks every target", () => {
  let harness: BehaviorLoopHarness;
  afterEach(async () => {
    await harness.cleanup();
  });

  it("never mutates any target under a stale prepared approval", async () => {
    harness = await createBehaviorLoopHarness({
      intelligence: true,
      mutation: true,
      unified: true,
    });
    await writeSceneFixture(harness, SCENE_FIXTURE);
    const scriptSha256 = sha256Of(
      "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tmove_and_slide()\n",
    );
    const prepared = await harness.unified!.prepareUnified(mixedTargets(scriptSha256));
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    // External change to the scene target before apply (rev_B).
    await writeFile(
      join(harness.workspace.root, "scenes/player.tscn"),
      SCENE_FIXTURE.replace("visible = true", "visible = false"),
      "utf8",
    );
    const applied = await harness.unified!.applyUnified({
      changeSetId: prepared.changeSet.id,
      approvedDigest: prepared.changeSet.combinedDigest,
    });
    expect(applied.status).toBe("conflict");
    // No target was mutated and no checkpoint was created.
    const script = await readWorkspaceFile(harness.workspace.root, "scripts/player/player.gd");
    expect(script).not.toContain("Vector2.UP");
    const scene = await readWorkspaceFile(harness.workspace.root, "scenes/player.tscn");
    expect(scene).toContain("visible = false");
    expect(scene).not.toContain("sprint_speed");
    expect(await harness.store.list()).toHaveLength(0);
  });
});

describe("S3M11 effect — review repair requires fresh artifacts", () => {
  let harness: BehaviorLoopHarness;
  afterEach(async () => {
    await harness.cleanup();
  });

  it("a blocking finding preserves prior changes and the repair needs a fresh approval", async () => {
    harness = await createBehaviorLoopHarness({
      intelligence: true,
      mutation: true,
      unified: true,
    });
    await writeSceneFixture(harness, SCENE_FIXTURE);
    const scriptSha256 = sha256Of(
      "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tmove_and_slide()\n",
    );
    harness.startUnifiedTask("Add a sprint property and configure the scene value", "mixed");

    const first = await harness.unified!.prepareUnified(mixedTargets(scriptSha256));
    expect(first.status).toBe("ready");
    if (first.status !== "ready") {
      return;
    }
    const firstApply = await harness.unified!.applyUnified({
      changeSetId: first.changeSet.id,
      approvedDigest: first.changeSet.combinedDigest,
    });
    expect(firstApply.status).toBe("applied");

    // Deterministic blocking reviewer finding.
    harness.unifiedEmit({ type: "quality_started", developmentId: "unified" });
    harness.unifiedEmit({ type: "review_started", developmentId: "unified" });
    harness.unifiedEmit({
      type: "review_completed",
      developmentId: "unified",
      critical: 1,
      high: 0,
      medium: 0,
      low: 0,
    });
    harness.unifiedEmit({
      type: "quality_completed",
      developmentId: "unified",
      status: "blocking_findings",
    });
    const current = harness.runtime.getTask(harness.runtime.latestTask()!.taskId)!.snapshot();
    expect(current.reviewStatus).toBe("findings");

    // The reviewer never mutated: prior changes stay on disk, and no
    // rollback happened automatically.
    const script = await readWorkspaceFile(harness.workspace.root, "scripts/player/player.gd");
    expect(script).toContain("move_and_slide(Vector2.UP)");

    // Bounded repair: fresh preparation produces a new identity; the old
    // approval is structurally unusable; the fresh approval succeeds.
    const repaired = await harness.unified!.prepareUnified({
      request: "Add a sprint property and configure the scene value (repair)",
      targets: [
        {
          kind: "text",
          changes: {
            changes: [
              {
                operation: "edit",
                path: "scripts/player/player.gd",
                expectedSha256: sha256Of(script),
                replacements: [
                  {
                    oldText: "move_and_slide(Vector2.UP)",
                    newText: "move_and_slide(Vector2.UP) # repaired",
                  },
                ],
              },
            ],
          },
        },
      ],
    });
    expect(repaired.status).toBe("ready");
    if (repaired.status !== "ready") {
      return;
    }
    expect(repaired.changeSet.combinedDigest).not.toBe(first.changeSet.combinedDigest);
    // Stale repair artifact: the old approval cannot be reused.
    const staleApply = await harness.unified!.applyUnified({
      changeSetId: repaired.changeSet.id,
      approvedDigest: first.changeSet.combinedDigest,
    });
    expect(staleApply.status).toBe("conflict");
    // Fresh approval applies the repair against the current revisions.
    const repairApply = await harness.unified!.applyUnified({
      changeSetId: repaired.changeSet.id,
      approvedDigest: repaired.changeSet.combinedDigest,
    });
    expect(repairApply.status).toBe("applied");
    const repairedScript = await readWorkspaceFile(
      harness.workspace.root,
      "scripts/player/player.gd",
    );
    expect(repairedScript).toContain("# repaired");

    // Fresh holistic re-review passes and the task completes.
    for (const event of cleanReviewEvents()) {
      harness.unifiedEmit(event);
    }
    const finalTask = harness.finishUnifiedTask("completed");
    expect(finalTask?.phase).toBe("completed");
  });
});

describe("S3M11 effect — acceptance integrity", () => {
  let harness: BehaviorLoopHarness;
  afterEach(async () => {
    await harness.cleanup();
  });

  it("an executor completion claim with a missing required validation never completes", async () => {
    harness = await createBehaviorLoopHarness({
      intelligence: true,
      mutation: true,
      unified: true,
    });
    // The scene attaches a script that does not exist: static
    // cross-surface consistency reports a concern.
    await writeSceneFixture(harness, SCENE_MISSING_SCRIPT);
    const scriptSha256 = sha256Of(
      "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tmove_and_slide()\n",
    );
    harness.startUnifiedTask("Add a sprint property and configure the scene value", "mixed");
    const prepared = await harness.unified!.prepareUnified(mixedTargets(scriptSha256));
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const applied = await harness.unified!.applyUnified({
      changeSetId: prepared.changeSet.id,
      approvedDigest: prepared.changeSet.combinedDigest,
    });
    expect(applied.status).toBe("applied");
    if (applied.status !== "applied") {
      return;
    }
    expect(applied.consistency.consistent).toBe(false);
    // The implementer reports "Task complete" while the cross-surface
    // consistency criterion was never verified.
    for (const event of cleanReviewEvents()) {
      harness.unifiedEmit(event);
    }
    const finalTask = harness.finishUnifiedTask("completed");
    expect(finalTask).not.toBeNull();
    if (finalTask === null) {
      return;
    }
    expect(finalTask.phase).toBe("blocked");
    expect(finalTask.phase).not.toBe("completed");
    const completion = harness.runtime.getTask(finalTask.taskId)?.evaluateCompletion();
    expect(completion?.allowed).toBe(false);
    expect(
      finalTask.acceptance.find((criterion) => criterion.criterionId === "cross-surface-consistent")
        ?.status,
    ).toBe("pending");
    // Successful prior changes are preserved (never auto-reverted).
    const scene = await readWorkspaceFile(harness.workspace.root, "scenes/player.tscn");
    expect(scene).toContain("sprint_speed = 140.5");
  });
});

describe("S3M11 effect — undo compatibility for mixed tasks", () => {
  let harness: BehaviorLoopHarness;
  afterEach(async () => {
    await harness.cleanup();
  });

  it("checkpoints carry hash-safe restoration preimages for every surface", async () => {
    harness = await createBehaviorLoopHarness({
      intelligence: true,
      mutation: true,
      unified: true,
    });
    await writeSceneFixture(harness, SCENE_FIXTURE);
    const scriptSha256 = sha256Of(
      "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tmove_and_slide()\n",
    );
    const prepared = await harness.unified!.prepareUnified(mixedTargets(scriptSha256));
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const applied = await harness.unified!.applyUnified({
      changeSetId: prepared.changeSet.id,
      approvedDigest: prepared.changeSet.combinedDigest,
    });
    expect(applied.status).toBe("applied");
    if (applied.status !== "applied") {
      return;
    }
    // Every changed file has a checkpoint whose preimage restores the
    // exact pre-mutation bytes (hash-verified on load by the store).
    const entries = await harness.store.list();
    expect(entries.length).toBeGreaterThanOrEqual(2);
    const preimages = new Map<string, string>();
    for (const entry of entries) {
      const preimage = await harness.store.loadPreimage(entry.id);
      expect(preimage).not.toBeNull();
      preimages.set(entry.relativePath, new TextDecoder().decode(preimage ?? new Uint8Array()));
    }
    expect(preimages.get("scripts/player/player.gd")).toContain("move_and_slide()");
    expect(preimages.get("scenes/player.tscn")).toContain("[gd_scene");
  });
});

/** Keep the TaskState type referenced (finalTask narrowing). */
export type { TaskState };

function completionMissing(harness: BehaviorLoopHarness, task: TaskState): string {
  const handle = harness.runtime.getTask(task.taskId);
  if (handle === null) {
    return "task handle missing";
  }
  const completion = handle.evaluateCompletion();
  return completion.allowed ? "allowed" : completion.missing.join("; ");
}
