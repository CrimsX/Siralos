import type { KnowledgeCandidate } from "./knowledge-model.js";

/**
 * Deterministic initial project-knowledge seeding (Stage 3 milestone 4 §42).
 *
 * Seeding uses existing static project discovery to create a few
 * high-confidence facts and nothing more: no broad architecture inference,
 * no design-intent guesses. Architectural ownership facts ("Player owns
 * navigation") require strong explicit evidence or user confirmation and
 * are never auto-seeded. All candidates flow through the single-writer
 * KnowledgeCoordinator like any other proposal.
 */

export interface GodotProjectKnowledgeSeed {
  /** SHA-256 of the exact project.godot file state the facts cite. */
  readonly projectFileSha256: string | null;
  /** Raw declared engine feature, e.g. "4.7" or "4.7.1". */
  readonly declaredEngineVersionRaw: string | null;
  /** "gdscript" | "dotnet" | "mixed" | "unknown" (null when unknown). */
  readonly languageProfile: string | null;
  /** True when a .NET project/solution file was statically detected. */
  readonly hasDotnet: boolean;
  /** Project display name from config/name, when declared. */
  readonly projectName: string | null;
}

const PROJECT_FILE_PATH = "project.godot";

/**
 * Build the conservative seed candidate set. Facts with no reliable
 * evidence are simply absent — the coordinator stores only what discovery
 * actually proved.
 */
export function buildGodotProjectKnowledgeCandidates(
  seed: GodotProjectKnowledgeSeed,
): readonly KnowledgeCandidate[] {
  const candidates: KnowledgeCandidate[] = [];
  const fileProvenance =
    seed.projectFileSha256 === null
      ? []
      : ([
          {
            type: "workspace_file",
            path: PROJECT_FILE_PATH,
            sha256: seed.projectFileSha256,
          },
        ] as const);

  if (seed.declaredEngineVersionRaw !== null) {
    candidates.push({
      subjectKey: "project.godot.version",
      type: "fact",
      content: seed.declaredEngineVersionRaw,
      provenance: [...fileProvenance],
      proposedConfidence: "high",
      proposedVolatility: "stable",
    });
  }

  if (seed.languageProfile !== null && seed.languageProfile !== "unknown") {
    candidates.push({
      subjectKey: "project.language_profile",
      type: "fact",
      content: seed.languageProfile,
      provenance: [...fileProvenance],
      proposedConfidence: "high",
      proposedVolatility: "stable",
    });
  }

  candidates.push({
    subjectKey: "project.has_dotnet",
    type: "fact",
    content: seed.hasDotnet ? "true" : "false",
    provenance: [...fileProvenance],
    proposedConfidence: "high",
    proposedVolatility: "stable",
  });

  if (seed.projectName !== null) {
    candidates.push({
      subjectKey: "project.name",
      type: "fact",
      content: seed.projectName,
      provenance: [...fileProvenance],
      proposedConfidence: "medium",
      proposedVolatility: "normal",
    });
  }

  return candidates;
}
