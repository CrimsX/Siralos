import { describe, expect, it } from "vitest";
import { createGodotRelationshipIndex } from "../../index.js";
import type { GodotRelationshipEntry } from "./relationship-index.js";

function entry(overrides: Partial<GodotRelationshipEntry>): GodotRelationshipEntry {
  return {
    sourcePath: "scenes/player.tscn",
    sourceRevision: "rev_a",
    kind: "scene_uses_script",
    targetPath: "scripts/player.gd",
    ...overrides,
  };
}

describe("createGodotRelationshipIndex", () => {
  it("records dependencies and referrers", () => {
    const index = createGodotRelationshipIndex();
    index.record("scenes/player.tscn", [
      entry({ kind: "scene_uses_script", targetPath: "scripts/player.gd" }),
      entry({ kind: "scene_instances", targetPath: "scenes/weapon.tscn" }),
    ]);
    index.record("scenes/weapon.tscn", [
      entry({
        sourcePath: "scenes/weapon.tscn",
        sourceRevision: "rev_w",
        kind: "scene_uses_script",
        targetPath: "scripts/weapon.gd",
      }),
    ]);
    expect(index.dependenciesOf("scenes/player.tscn")).toHaveLength(2);
    expect(index.referrersOf("scripts/player.gd").map((e) => e.sourcePath)).toEqual([
      "scenes/player.tscn",
    ]);
    expect(index.size).toBe(3);
  });

  it("replaces all entries of a source on reparse", () => {
    const index = createGodotRelationshipIndex();
    index.record("scenes/player.tscn", [
      entry({ sourceRevision: "rev_a", kind: "scene_uses_script", targetPath: "scripts/old.gd" }),
    ]);
    index.record("scenes/player.tscn", [
      entry({ sourceRevision: "rev_b", kind: "scene_uses_script", targetPath: "scripts/new.gd" }),
    ]);
    expect(index.dependenciesOf("scenes/player.tscn")).toHaveLength(1);
    expect(index.dependenciesOf("scenes/player.tscn")[0]!.targetPath).toBe("scripts/new.gd");
    // The stale derivation is gone from the referrer side too.
    expect(index.referrersOf("scripts/old.gd")).toHaveLength(0);
    expect(index.size).toBe(1);
  });

  it("flags entries whose recorded revision is not current as stale", () => {
    const index = createGodotRelationshipIndex();
    const recorded = entry({ sourceRevision: "rev_a" });
    index.record("scenes/player.tscn", [recorded]);
    const current = index.dependenciesOf("scenes/player.tscn")[0]!;
    expect(index.isStale(current, "rev_a")).toBe(false);
    expect(index.isStale(current, "rev_b")).toBe(true);
    // An unknown current revision (file invalidated/never seen) is stale.
    expect(index.isStale(current, null)).toBe(true);
  });

  it("never presents a stale relationship as current through referrers", () => {
    const index = createGodotRelationshipIndex();
    index.record("scenes/player.tscn", [
      entry({
        sourceRevision: "rev_a",
        kind: "scene_uses_script",
        targetPath: "scripts/player.gd",
      }),
    ]);
    // External modification: the registry (represented here by the caller)
    // now knows rev_b for player.tscn.
    const referrer = index.referrersOf("scripts/player.gd")[0]!;
    expect(index.isStale(referrer, "rev_b")).toBe(true);
  });

  it("bounded: evicts oldest sources past the entry limit", () => {
    const index = createGodotRelationshipIndex({ maxEntries: 3 });
    index.record("a.tscn", [
      entry({ sourcePath: "a.tscn", kind: "scene_uses_script", targetPath: "a.gd" }),
    ]);
    index.record("b.tscn", [
      entry({ sourcePath: "b.tscn", kind: "scene_uses_script", targetPath: "b.gd" }),
    ]);
    index.record("c.tscn", [
      entry({ sourcePath: "c.tscn", kind: "scene_uses_script", targetPath: "c.gd" }),
    ]);
    index.record("d.tscn", [
      entry({ sourcePath: "d.tscn", kind: "scene_uses_script", targetPath: "d.gd" }),
    ]);
    expect(index.size).toBe(3);
    expect(index.dependenciesOf("a.tscn")).toHaveLength(0);
    expect(index.referrersOf("a.gd")).toHaveLength(0);
    expect(index.dependenciesOf("d.tscn")).toHaveLength(1);
  });

  it("retains uid identity on entries when both path and uid are known", () => {
    const index = createGodotRelationshipIndex();
    index.record("scenes/player.tscn", [
      entry({ kind: "scene_inherits", targetPath: "scenes/base.tscn", targetUid: "uid://base001" }),
    ]);
    const inherited = index.dependenciesOf("scenes/player.tscn")[0]!;
    expect(inherited.targetUid).toBe("uid://base001");
    expect(inherited.kind).toBe("scene_inherits");
  });

  it("clears all state", () => {
    const index = createGodotRelationshipIndex();
    index.record("a.tscn", [
      entry({ sourcePath: "a.tscn", kind: "scene_uses_script", targetPath: "a.gd" }),
    ]);
    index.clear();
    expect(index.size).toBe(0);
    expect(index.dependenciesOf("a.tscn")).toHaveLength(0);
  });
});
