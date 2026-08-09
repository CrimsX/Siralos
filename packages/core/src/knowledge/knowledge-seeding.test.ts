import { describe, expect, it } from "vitest";
import { buildGodotProjectKnowledgeCandidates } from "./knowledge-seeding.js";

describe("Godot project knowledge seeding (conservative)", () => {
  it("seeds only facts the static profile proves", () => {
    const candidates = buildGodotProjectKnowledgeCandidates({
      projectFileSha256: "a".repeat(64),
      declaredEngineVersionRaw: "4.7",
      languageProfile: "gdscript",
      hasDotnet: false,
      projectName: "My Game",
    });
    const subjects = candidates.map((candidate) => candidate.subjectKey);
    expect(subjects).toContain("project.godot.version");
    expect(subjects).toContain("project.language_profile");
    expect(subjects).toContain("project.has_dotnet");
    expect(subjects).toContain("project.name");
    const version = candidates.find(
      (candidate) => candidate.subjectKey === "project.godot.version",
    );
    expect(version?.content).toBe("4.7");
    expect(version?.proposedConfidence).toBe("high");
    expect(version?.proposedVolatility).toBe("stable");
    expect(version?.provenance).toEqual([
      { type: "workspace_file", path: "project.godot", sha256: "a".repeat(64) },
    ]);
  });

  it("omits unknown engine version and language profile", () => {
    const candidates = buildGodotProjectKnowledgeCandidates({
      projectFileSha256: null,
      declaredEngineVersionRaw: null,
      languageProfile: null,
      hasDotnet: false,
      projectName: null,
    });
    expect(candidates.map((candidate) => candidate.subjectKey)).toEqual(["project.has_dotnet"]);
  });

  it("records has_dotnet from static .NET detection", () => {
    const dotnet = buildGodotProjectKnowledgeCandidates({
      projectFileSha256: "b".repeat(64),
      declaredEngineVersionRaw: null,
      languageProfile: "dotnet",
      hasDotnet: true,
      projectName: null,
    });
    const fact = dotnet.find((candidate) => candidate.subjectKey === "project.has_dotnet");
    expect(fact?.content).toBe("true");
  });

  it("never infers design intent from weak evidence", () => {
    const candidates = buildGodotProjectKnowledgeCandidates({
      projectFileSha256: null,
      declaredEngineVersionRaw: null,
      languageProfile: null,
      hasDotnet: false,
      projectName: null,
    });
    // No architectural-ownership candidates are ever auto-seeded.
    expect(candidates.some((candidate) => candidate.content.includes("owns"))).toBe(false);
  });
});
