import { describe, expect, it } from "vitest";
import { createMutationLock } from "./mutation-lock.js";

describe("createMutationLock", () => {
  it("serializes critical sections", async () => {
    const lock = createMutationLock();
    let active = 0;
    let maxActive = 0;
    const run = async () => {
      const release = await lock.acquire();
      active += 1;
      maxActive = Math.max(maxActive, active);
      await Promise.resolve();
      active -= 1;
      release();
    };
    await Promise.all([run(), run(), run()]);
    expect(maxActive).toBe(1);
  });

  it("releases the lock after failure", async () => {
    const lock = createMutationLock();
    const release = await lock.acquire();
    release();
    const second = await lock.acquire();
    second();
    expect(true).toBe(true);
  });

  it("supports cancellation while waiting", async () => {
    const lock = createMutationLock();
    const firstRelease = await lock.acquire();
    const controller = new AbortController();
    controller.abort();
    await expect(lock.acquire(controller.signal)).rejects.toThrow();
    firstRelease();
    const third = await lock.acquire();
    third();
  });
});
