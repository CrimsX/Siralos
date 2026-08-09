import { describe, expect, it } from "vitest";
import { createKnowledgeCoordinator } from "./knowledge-coordinator.js";

const researchRef = {
  type: "research_evidence" as const,
  evidenceId: "ev-research-1",
  source: { kind: "godot-docs" as const, id: "godot-docs", label: "Godot documentation" },
  fetchedAtMs: 1_700_000_000_000,
};

describe("research evidence provenance (milestone 5)", () => {
  it("accepts research_evidence provenance when the host verifies the evidence", () => {
    const coordinator = createKnowledgeCoordinator({
      hasResearchEvidence: (evidenceId) => evidenceId === "ev-research-1",
    });
    const result = coordinator.propose({
      subjectKey: "godot.signals.best_practice",
      content: "Signals are preferred over polling for cross-node updates.",
      provenance: [researchRef],
      proposedConfidence: "medium",
    });
    expect(result.status).toBe("accepted");
    if (result.status === "accepted") {
      expect(result.fact.provenance[0]?.type).toBe("research_evidence");
    }
  });

  it("rejects research_evidence provenance for unknown evidence ids", () => {
    const coordinator = createKnowledgeCoordinator({
      hasResearchEvidence: (evidenceId) => evidenceId === "ev-research-1",
    });
    const result = coordinator.propose({
      subjectKey: "godot.signals.best_practice",
      content: "Signals are preferred over polling.",
      provenance: [{ ...researchRef, evidenceId: "ev-research-999" }],
    });
    expect(result.status).toBe("rejected");
    if (result.status === "rejected") {
      expect(result.reason).toContain("does not exist");
      expect(result.reason).toContain("ev-research-999");
    }
  });

  it("rejects research_evidence provenance when no verifier is configured", () => {
    const coordinator = createKnowledgeCoordinator();
    const result = coordinator.propose({
      subjectKey: "godot.signals.best_practice",
      content: "Signals are preferred over polling.",
      provenance: [researchRef],
    });
    expect(result.status).toBe("rejected");
    if (result.status === "rejected") {
      expect(result.reason).toContain("verifier");
    }
  });

  it("rejects malformed research_evidence provenance", () => {
    const coordinator = createKnowledgeCoordinator({
      hasResearchEvidence: () => true,
    });
    const badSource = coordinator.propose({
      subjectKey: "godot.signals.best_practice",
      content: "Signals are preferred over polling.",
      provenance: [{ ...researchRef, source: { kind: "fake", id: "", label: "x" } }],
    });
    expect(badSource.status).toBe("rejected");
    const badTimestamp = coordinator.propose({
      subjectKey: "godot.signals.best_practice",
      content: "Signals are preferred over polling.",
      provenance: [{ ...researchRef, fetchedAtMs: Number.NaN }],
    });
    expect(badTimestamp.status).toBe("rejected");
    const emptyId = coordinator.propose({
      subjectKey: "godot.signals.best_practice",
      content: "Signals are preferred over polling.",
      provenance: [{ ...researchRef, evidenceId: "" }],
    });
    expect(emptyId.status).toBe("rejected");
  });

  it("never proposes research facts automatically (facts only via explicit propose)", () => {
    const coordinator = createKnowledgeCoordinator({
      hasResearchEvidence: () => true,
    });
    // Nothing has been proposed; research evidence alone never creates facts.
    expect(coordinator.fact("godot.signals.best_practice")).toBeNull();
    expect(coordinator.pinnedFacts()).toHaveLength(0);
  });

  it("continues to accept non-research provenance without a research verifier", () => {
    const coordinator = createKnowledgeCoordinator({
      hasEvidence: (evidenceId) => evidenceId === "ev-1",
    });
    const result = coordinator.propose({
      subjectKey: "project.test.framework",
      content: "GUT",
      provenance: [{ type: "evidence", evidenceId: "ev-1", kind: "workspace_read" }],
    });
    expect(result.status).toBe("accepted");
  });
});
