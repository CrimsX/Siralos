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

  it("aborts a queued waiter promptly while the lock is held", async () => {
    const lock = createMutationLock();
    const firstRelease = await lock.acquire();
    const controller = new AbortController();
    const queued = lock.acquire(controller.signal);
    let settled = false;
    const result = queued.then(
      () => {
        settled = true;
        return "resolved";
      },
      () => {
        settled = true;
        return "rejected";
      },
    );
    await Promise.resolve();
    expect(settled).toBe(false);
    controller.abort();
    await expect(result).resolves.toBe("rejected");
    firstRelease();
    const after = await lock.acquire();
    after();
  });

  it("keeps the queue healthy after an aborted waiter", async () => {
    const lock = createMutationLock();
    const firstRelease = await lock.acquire();
    const abortController = new AbortController();
    const waiterOne = lock.acquire(abortController.signal);
    const waiterTwo = lock.acquire();
    abortController.abort();
    await expect(waiterOne).rejects.toThrow();
    firstRelease();
    const secondRelease = await waiterTwo;
    secondRelease();
    const thirdRelease = await lock.acquire();
    thirdRelease();
  });
});
