import { describe, expect, it } from "vitest";
import { mkdtemp, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ReferenceCacheMetadata } from "./reference-cache.js";
import { createInMemoryCacheStore, createReferenceCacheStore } from "./reference-cache.js";

function sampleMetadata(fingerprint = "fp-1"): ReferenceCacheMetadata {
  return {
    fingerprint,
    source: { kind: "local-directory", path: "/some/root" },
    resolvedRevision: {
      identity: { kind: "local-directory", canonicalPath: "/some/root", fingerprint },
      resolvedAtMs: 123,
    },
    lastAccessMs: 456,
    sizeBytes: 1024,
  };
}

describe("createReferenceCacheStore (real store)", () => {
  it("is a fail-closed no-op: load misses, store unavailable, status unavailable", async () => {
    const store = createReferenceCacheStore();
    const status = store.status();
    expect(status).toMatchObject({ status: "unavailable" });
    if (status.status === "unavailable") {
      expect(status.reason).toBeTypeOf("string");
    }
    expect(await store.load("fp")).toBeNull();
    expect(await store.store(sampleMetadata())).toMatchObject({ status: "unavailable" });
  });

  it("performs zero filesystem operations", async () => {
    const root = await mkdtemp(join(tmpdir(), "siralos-cache-test-"));
    try {
      const store = createReferenceCacheStore({ root });
      await store.store(sampleMetadata());
      await store.load("fp");
      store.status();
      expect(await readdir(root)).toEqual([]);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});

describe("createInMemoryCacheStore", () => {
  it("round-trips metadata", async () => {
    const store = createInMemoryCacheStore();
    expect(store.status()).toEqual({ status: "enabled" });
    expect(await store.load("fp-1")).toBeNull();
    expect(await store.store(sampleMetadata("fp-1"))).toEqual({ status: "ok" });
    expect(await store.load("fp-1")).toEqual(sampleMetadata("fp-1"));
    expect(await store.load("fp-2")).toBeNull();
  });

  it("overwrites on re-store", async () => {
    const store = createInMemoryCacheStore();
    await store.store(sampleMetadata("fp-1"));
    const updated = { ...sampleMetadata("fp-1"), sizeBytes: 2048 };
    await store.store(updated);
    expect((await store.load("fp-1"))?.sizeBytes).toBe(2048);
  });
});
