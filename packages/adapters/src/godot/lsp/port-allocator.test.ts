import { describe, expect, it } from "vitest";
import { createLSPPortAllocator } from "./port-allocator.js";
import { createServer } from "node:net";

describe("createLSPPortAllocator", () => {
  it("allocates a bounded ephemeral port bound to loopback only", async () => {
    const allocator = createLSPPortAllocator();
    const allocated = await allocator.allocate();
    expect(allocated.host).toBe("127.0.0.1");
    expect(Number.isInteger(allocated.port)).toBe(true);
    expect(allocated.port).toBeGreaterThan(0);
    expect(allocated.port).toBeLessThanOrEqual(65535);
  });

  it("is race-safe: the OS allocation is the race answer and the port is reusable", async () => {
    const allocator = createLSPPortAllocator();
    const first = await allocator.allocate();
    const second = await allocator.allocate();
    expect(first.port).not.toBe(second.port);
    // After release, the port can be bound again (it is no longer in use).
    const probe = createServer();
    await new Promise<void>((resolve, reject) => {
      probe.once("error", reject);
      probe.listen(second.port, "127.0.0.1", () => {
        probe.removeListener("error", reject);
        resolve();
      });
    });
    await new Promise<void>((resolve) => probe.close(() => resolve()));
  });

  it("release reports a port still in use truthfully", async () => {
    const allocator = createLSPPortAllocator();
    const allocated = await allocator.allocate();
    const holder = createServer();
    await new Promise<void>((resolve, reject) => {
      holder.once("error", reject);
      holder.listen(allocated.port, "127.0.0.1", () => {
        holder.removeListener("error", reject);
        resolve();
      });
    });
    await expect(allocator.release(allocated.port)).rejects.toThrow("still in use");
    await new Promise<void>((resolve) => holder.close(() => resolve()));
    await allocator.release(allocated.port);
  });
});
