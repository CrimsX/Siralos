import { describe, expect, it } from "vitest";
import { validateProjectContext } from "./check-project-context.mjs";

function rows(prefix, count, width) {
  return Array.from(
    { length: count },
    (_, index) => `| ${prefix}-${String(index + 1).padStart(width, "0")} | item |`,
  ).join("\n");
}

function validInput() {
  const sections = [
    "Product definition",
    "Current implementation reality",
    "Current roadmap position",
    "Stage 3R migration track",
    "Rust engineering direction",
    "Architecture",
    "Permanent security model",
    "Mutation model",
    "Context model",
    "Provider model",
    "Godot domain policy",
    "Domain host boundary",
    "R2 differential contract",
    "H1 / H2 / ICM / H3",
    "Stage 4 direction",
    "Stage 5 / 6 guardrails",
    "Harness-derived design lessons",
    "Anti-patterns",
    "Verification model",
    "Authoritative documentation index",
    "Prompt / goal generation rules",
    "New-session bootstrap",
  ];
  return {
    context: [
      "Project: Siralos",
      "Context schema: 1",
      "Status: Active development",
      "Public stages: 6",
      "Migration track: Stage 3R",
      "Current completed milestone: R5",
      "Next milestone: R6 - Minimal Domain Capability Architecture and Synthetic Conformance Domain",
      `Last verified commit: ${"a".repeat(40)}`,
      "Canonical repository: https://github.com/CrimsX/Siralos",
      "R4      COMPLETE",
      "R5      COMPLETE",
      "R6      NEXT",
      ...sections.map((section, index) => `## ${index + 1}. ${section}`),
    ].join("\n"),
    agents: "Read docs/development/PROJECT_CONTEXT.md first.",
    requirements: [rows("CORE", 20, 3), rows("HAR", 55, 3), rows("AP", 16, 3)].join("\n"),
    rfc: rows("RFC", 20, 4),
    golden: rows("GT", 18, 3),
    commitExists: () => true,
  };
}

describe("validateProjectContext", () => {
  it("accepts the complete bootstrap and exact registries", () => {
    expect(validateProjectContext(validInput())).toEqual([]);
  });

  it("rejects a missing stable ID without inventing a substitute", () => {
    const input = validInput();
    input.requirements = input.requirements.replace("| HAR-055 | item |", "");

    expect(validateProjectContext(input)).toContain(
      "harness registry must contain its exact ordered ID range once",
    );
  });

  it("rejects an unverifiable commit pointer", () => {
    const input = validInput();
    input.commitExists = () => false;

    expect(validateProjectContext(input)).toContain(
      "Last verified commit does not resolve to a commit",
    );
  });
});
