import type {
  GodotSceneIntelligence,
  Tool,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@solaris/core";
import { readJsonObject, readRequiredString } from "../../tools/workspace/validation.js";

/**
 * Read-only `godot.dependencies` provider tool (Stage 3 milestone 8).
 *
 * One bounded query: immediate dependencies of a scene/resource plus a
 * bounded, cycle-safe traversal (depth/file bounds) and — from the
 * relationship index — who references the target. No broad graph-query
 * DSL, no eager project-wide expansion. Read-only: no Godot process, no
 * project code, no mutation.
 */
export function createGodotDependenciesTool(intelligence: GodotSceneIntelligence): Tool {
  return {
    definition: {
      name: "godot.dependencies",
      description:
        "Bounded dependency query for one .tscn/.tres: immediate dependencies (inheritance, instances, scripts, external resources), a bounded cycle-safe traversal with depth/file limits, and who references the target (from the parsed-relationship index). Read-only; large or cyclic graphs are reported as bounded, never expanded fully.",
      inputSchema: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: "Workspace-relative .tscn or .tres path, e.g. scenes/player.tscn.",
          },
        },
        required: ["path"],
        additionalProperties: false,
      },
    },
    capability: "godot.inspect",
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Inspection was cancelled." };
      }
      const parsed = readJsonObject(input);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      const parsedPath = readRequiredString(parsed.value, "path");
      if (!parsedPath.ok) {
        return { status: "invalid_input", message: parsedPath.message };
      }
      const result = await intelligence.dependencies({ path: parsedPath.value });
      if (result.status !== "ok") {
        return {
          status: result.status === "denied" ? "denied" : "failed",
          message: result.message ?? `Dependency query failed (${result.status}).`,
        };
      }
      const maxListItems = 128;
      const edges = result.edges.slice(0, maxListItems);
      const referrers = result.referrers.slice(0, maxListItems);
      return {
        status: "success",
        output: {
          rootPath: result.rootPath,
          revision: result.revision,
          filesVisited: result.filesVisited,
          truncatedDepth: result.truncatedDepth,
          truncatedFiles: result.truncatedFiles,
          cycleDetected: result.cycleDetected,
          ...(result.cyclePath === undefined ? {} : { cyclePath: result.cyclePath }),
          edges: edges.map((edge) => ({
            kind: edge.kind,
            sourcePath: edge.sourcePath,
            targetPath: edge.targetPath,
            ...(edge.targetUid === undefined ? {} : { targetUid: edge.targetUid }),
            depth: edge.depth,
          })),
          edgesTruncated: result.edges.length > maxListItems,
          referrers: referrers.map((referrer) => ({
            sourcePath: referrer.sourcePath,
            kind: referrer.kind,
            sourceRevision: referrer.sourceRevision,
            stale: referrer.stale,
          })),
          referrersTruncated: result.referrers.length > maxListItems,
        } as never,
        summary: `${result.rootPath}: ${result.edges.length} edges, ${result.filesVisited} files visited${result.cycleDetected ? " (cycle)" : ""}`,
      };
    },
  };
}
