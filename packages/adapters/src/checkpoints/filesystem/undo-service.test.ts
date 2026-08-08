import { describe, expect, it } from "vitest";
import { createUndoService, UNDO_UNAVAILABLE_MESSAGE } from "./undo-service.js";

describe("undo service fail-closed availability", () => {
  it("is unavailable before any filesystem activity, approval, or state change", async () => {
    const service = createUndoService({
      workspaceRoot: "/workspace",
      store: {
        get(): Promise<never> {
          return Promise.reject(new Error("store must not be reached"));
        },
        list(): Promise<never> {
          return Promise.reject(new Error("store must not be reached"));
        },
      } as never,
      lock: {} as never,
      reviewer: {} as never,
    });
    const outcome = await service.undo();
    expect(outcome.type).toBe("failed");
    if (outcome.type === "failed") {
      expect(outcome.message).toBe(UNDO_UNAVAILABLE_MESSAGE);
      expect(outcome.checkpointId).toBeNull();
      expect(outcome.path).toBeNull();
    }
  });

  it("is unavailable for a specific checkpoint id too", async () => {
    const service = createUndoService({
      workspaceRoot: "/workspace",
      store: {} as never,
      lock: {} as never,
      reviewer: {} as never,
    });
    const outcome = await service.undo("cp_anything");
    expect(outcome.type).toBe("failed");
  });
});
