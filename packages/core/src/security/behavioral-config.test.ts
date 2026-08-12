import { describe, expect, it } from "vitest";
import {
  classifyBehavioralConfigPaths,
  isProtectedBehavioralConfigPath,
} from "./behavioral-config.js";

describe("protected behavioral configuration paths", () => {
  it("protects AGENTS.md at the workspace root", () => {
    expect(isProtectedBehavioralConfigPath("AGENTS.md")).toBe(true);
  });

  it("protects AGENTS.md at any depth", () => {
    expect(isProtectedBehavioralConfigPath("src/AGENTS.md")).toBe(true);
    expect(isProtectedBehavioralConfigPath("src/player/AGENTS.md")).toBe(true);
  });

  it("protects the .siralos directory tree", () => {
    expect(isProtectedBehavioralConfigPath(".siralos/config.json")).toBe(true);
    expect(isProtectedBehavioralConfigPath("src/.siralos/workflows/dev.json")).toBe(true);
    expect(isProtectedBehavioralConfigPath(".siralos")).toBe(true);
  });

  it("matches case-insensitively", () => {
    expect(isProtectedBehavioralConfigPath("agents.md")).toBe(true);
    expect(isProtectedBehavioralConfigPath("src/AgEnTs.MD")).toBe(true);
    expect(isProtectedBehavioralConfigPath(".SIRALOS/rules")).toBe(true);
  });

  it("does not protect ordinary source paths", () => {
    expect(isProtectedBehavioralConfigPath("src/player/player.gd")).toBe(false);
    expect(isProtectedBehavioralConfigPath("project.godot")).toBe(false);
    expect(isProtectedBehavioralConfigPath("addons/foo/README.md")).toBe(false);
    expect(isProtectedBehavioralConfigPath("agents_md.txt")).toBe(false);
  });

  it("classifies the protected members of a path set", () => {
    expect(
      classifyBehavioralConfigPaths([
        "src/player/player.gd",
        "AGENTS.md",
        ".siralos/config.json",
        "src/player/AGENTS.md",
      ]),
    ).toEqual(["AGENTS.md", ".siralos/config.json", "src/player/AGENTS.md"]);
  });
});
