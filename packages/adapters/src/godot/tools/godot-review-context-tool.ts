import type {
  GodotSceneIntelligence,
  Tool,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@solaris/core";
import {
  readArrayField,
  readJsonObject,
  readOptionalPositiveInteger,
  readRequiredString,
} from "../../tools/workspace/validation.js";

/**
 * Read-only `godot.review_context` provider tool (Stage 3 milestone 9).
 *
 * One bounded impact-analysis query: given changed workspace surfaces,
 * derive a `ReviewContextManifest` (primary/related surfaces with
 * verified vs candidate confidence, regression areas, validation
 * recommendations, evidence, completeness) from the existing
 * revision-aware relationship index. Read-only: no Godot process, no
 * project code, no mutation, no checkpoint.
 */
export function createGodotReviewContextTool(intelligence: GodotSceneIntelligence): Tool {
  return {
    definition: {
      name: "godot.review_context",
      description:
        "Bounded impact analysis for changed Godot surfaces (scripts, scenes, resources, autoloads, project.godot): derived ReviewContextManifest with primary changes, verified/candidate related surfaces (script attachments, inheritance, instancing, resource dependencies, serialized signal connections), regression areas, validation recommendations, and honest completeness. Read-only and revision-bound; candidate impact is never presented as verified.",
      inputSchema: {
        type: "object",
        properties: {
          taskId: {
            type: "string",
            description: "Task id for the derived manifest.",
          },
          taskContractRevision: {
            type: "integer",
            description: "TaskContract revision the manifest binds to.",
          },
          changedPaths: {
            type: "array",
            items: { type: "string" },
            description:
              "Workspace-relative changed surfaces, e.g. scripts/player.gd. Bounded (first 16).",
          },
        },
        required: ["taskId", "taskContractRevision", "changedPaths"],
        additionalProperties: false,
      },
    },
    capability: "godot.inspect",
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Impact analysis was cancelled." };
      }
      const parsed = readJsonObject(input);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      const taskId = readRequiredString(parsed.value, "taskId");
      if (!taskId.ok) {
        return { status: "invalid_input", message: taskId.message };
      }
      const taskContractRevision = readOptionalPositiveInteger(
        parsed.value,
        "taskContractRevision",
      );
      if (!taskContractRevision.ok) {
        return { status: "invalid_input", message: taskContractRevision.message };
      }
      const rawPaths = readArrayField(parsed.value, "changedPaths");
      if (!rawPaths.ok || rawPaths.value.length === 0) {
        return {
          status: "invalid_input",
          message: "changedPaths must be a non-empty array of workspace-relative paths.",
        };
      }
      const changedPaths: string[] = [];
      for (const entry of rawPaths.value) {
        if (typeof entry !== "string") {
          return { status: "invalid_input", message: "changedPaths entries must be strings." };
        }
        changedPaths.push(entry);
      }
      const result = await intelligence.reviewContext({
        taskId: taskId.value,
        taskContractRevision: taskContractRevision.value ?? 1,
        changedPaths,
      });
      if (result.status !== "ok" || result.manifest === null) {
        return {
          status: "failed",
          message: result.message ?? "Impact analysis failed.",
        };
      }
      const manifest = result.manifest;
      const relatedCount = manifest.relatedSurfaces.length;
      return {
        status: "success",
        summary: `Impact context for ${manifest.primaryChanges.length} changed surface(s): ${relatedCount} related surface(s), ${manifest.regressionAreas.length} regression area(s), completeness ${manifest.completeness}.`,
        output: {
          taskId: manifest.taskId,
          taskContractRevision: manifest.taskContractRevision,
          completeness: manifest.completeness,
          primaryChanges: manifest.primaryChanges.map((surface) => ({
            path: surface.path,
            kind: surface.kind,
            revision: surface.revision,
            confidence: surface.confidence,
            ...(surface.note === undefined ? {} : { note: surface.note }),
          })),
          relatedSurfaces: manifest.relatedSurfaces.map((relation) => ({
            kind: relation.kind,
            sourcePath: relation.sourcePath,
            targetPath: relation.targetPath,
            confidence: relation.confidence,
            ...(relation.note === undefined ? {} : { note: relation.note }),
          })),
          regressionAreas: manifest.regressionAreas.map((area) => ({
            id: area.id,
            title: area.title,
            reason: area.reason,
            surfaces: area.surfaces,
          })),
          validation: manifest.validation.map((recommendation) => ({
            kind: recommendation.kind,
            priority: recommendation.priority,
            rationale: recommendation.rationale,
            surfaces: recommendation.surfaces,
          })),
          diagnostics: manifest.diagnostics.map((diagnostic) => ({
            code: diagnostic.code,
            message: diagnostic.message,
          })),
          evidence: manifest.evidence,
        },
      };
    },
  };
}
