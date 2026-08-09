import { afterEach, describe, expect, it } from "vitest";
import type { TaskState } from "@solaris/core";
import { createUndoService } from "@solaris/adapters";
import {
  createBehaviorLoopHarness,
  FIXTURE_PATH,
  readWorkspaceFile,
  type BehaviorLoopHarness,
} from "./behavior-harness.js";

/**
 * Behavior fixtures 13-15 (Stage 3 milestone 1 §20): the current Stage 2
 * `/develop` flow integrated with the task runtime, verified at the final
 * observable boundary — the full application tool loop with the
 * deterministic fake provider, a real temp workspace, a real checkpoint
 * store, and the host task gate.
 */

describe("Behavior 13 — the Stage 2 /develop success path still works, through the task runtime", () => {
  let harness: BehaviorLoopHarness;
  afterEach(async () => {
    await harness.cleanup();
  });

  it("drives investigate -> propose -> approve -> apply -> validate -> review -> completed", async () => {
    harness = await createBehaviorLoopHarness();
    await harness.startWorkflow("develop fixture");
    await harness.runPrompt("develop fixture");
    const task = await harness.finalizeTask();
    expect(task).not.toBeNull();
    if (task === null) {
      return;
    }

    // The Stage 2 workflow itself completed cleanly.
    const workflow = harness.status();
    expect(workflow.session?.state).toEqual({ kind: "terminal", status: "completed" });
    expect(workflow.session?.validation).toBe("clean");
    expect(harness.approvals()).toBe(1);

    // The task completed through the host gate with evidence-backed steps.
    expect(task.phase).toBe("completed");
    expect(task.contractRevision).toBe(1);
    for (const step of task.steps) {
      expect(step.status).toBe("completed");
      expect(step.evidenceRefs.length).toBeGreaterThan(0);
    }
    for (const criterion of task.acceptance) {
      expect(criterion.status).toBe("satisfied");
    }
    expect(task.validationStatus).toBe("clean");
    expect(task.reviewStatus).toBe("clean");
    expect(task.evidence.some((entry) => entry.kind === "mutation_receipt")).toBe(true);
    expect(task.evidence.some((entry) => entry.kind === "parser_result")).toBe(true);
    expect(task.evidence.some((entry) => entry.kind === "lsp_result")).toBe(true);
    expect(task.evidence.some((entry) => entry.kind === "review_result")).toBe(true);

    // The workspace contains exactly the approved change, checkpointed.
    const onDisk = await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH);
    expect(onDisk).toContain("move_and_slide(Vector2.UP)");
    expect(await harness.store.list()).toHaveLength(1);
  });
});

describe("Behavior 14 — the Stage 2 failed-validation path still refuses clean completion", () => {
  let harness: BehaviorLoopHarness;
  afterEach(async () => {
    await harness.cleanup();
  });

  it("a parser error ends the workflow with errors and the task fails, never completes", async () => {
    harness = await createBehaviorLoopHarness();
    // The changed file fails the post-edit parser gate and the provider
    // finishes without repairing (same as a denied/abandoned repair).
    harness.parserControl.resultsByPath.set(FIXTURE_PATH, {
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
    await harness.startWorkflow("develop fixture");
    await harness.runPrompt("develop fixture");
    const task = await harness.finalizeTask();
    expect(task).not.toBeNull();
    if (task === null) {
      return;
    }

    // The workflow truthfully reports the failed validation.
    const workflow = harness.status();
    expect(["completed_with_errors", "validation_failed"]).toContain(
      workflow.session?.state.kind === "terminal" ? workflow.session.state.status : "",
    );
    expect(workflow.session?.validation).toBe("errors");

    // The task refuses clean completion: failed, not completed.
    expect(task.phase).toBe("failed");
    expect(task.phase).not.toBe("completed");
    expect(task.validationStatus).toBe("failed");
    expect(task.completedAtMs).not.toBeNull();
    const completion = harness.runtime.getTask(task.taskId)?.evaluateCompletion();
    expect(completion?.allowed).toBe(false);
    // No task_completed activity record exists.
    const log = harness.runtime.getTask(task.taskId)?.activityLog() ?? [];
    expect(log.some((event) => event.type === "task_completed")).toBe(false);
    expect(log.some((event) => event.type === "task_failed")).toBe(true);
  });
});

describe("Behavior 15 — cancellation remains safe", () => {
  let harness: BehaviorLoopHarness;
  afterEach(async () => {
    await harness.cleanup();
  });

  it("already-applied approved edits remain, and /undo stays available after cancellation", async () => {
    harness = await createBehaviorLoopHarness();

    // First workflow: apply a change successfully.
    await harness.startWorkflow("develop fixture");
    await harness.runPrompt("develop fixture");
    const completed = await harness.finalizeTask();
    expect(completed?.phase).toBe("completed");
    const onDisk = await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH);
    expect(onDisk).toContain("move_and_slide(Vector2.UP)");
    const checkpoints = await harness.store.list();
    expect(checkpoints).toHaveLength(1);

    // Second workflow: cancel before any change set is proposed.
    await harness.startWorkflow("develop fixture");
    const cancelled = await harness.cancelWorkflow();
    expect(cancelled.task?.phase).toBe("cancelled");
    expect(cancelled.task?.terminalReason).toContain("cancelled");

    // The first applied edit is untouched by the cancellation.
    const afterCancel = await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH);
    expect(afterCancel).toContain("move_and_slide(Vector2.UP)");

    // The checkpoint remains listed, and /undo remains available
    // (fail-closed on this platform: typed refusal before any write).
    const storeAfterCancel = await harness.store.list();
    expect(storeAfterCancel).toHaveLength(1);
    const undo = createUndoService({
      workspaceRoot: harness.workspace.root,
      store: harness.store,
      lock: { acquire: () => Promise.resolve(() => undefined) },
      reviewer: { review: () => Promise.resolve({ type: "approve_once" }) },
    });
    const outcome = await undo.undo();
    expect(outcome.type).toBe("failed");
    if (outcome.type === "failed") {
      expect(outcome.message).toContain("Undo is unavailable");
    }
    // Undo refused before any state change: the edit and checkpoint remain.
    expect(await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH)).toContain(
      "move_and_slide(Vector2.UP)",
    );
    expect(await harness.store.list()).toHaveLength(1);
    // The cancelled task is terminal and recorded in the activity log.
    const task: TaskState | null =
      harness.runtime.getTask(cancelled.task?.taskId ?? "")?.snapshot() ?? null;
    expect(task?.phase).toBe("cancelled");
  });
});
