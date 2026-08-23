import type { GDScriptStructure } from "./gdscript-structure.js";
import type { WorkspaceRevisionHandle } from "./workspace-revision.js";

/**
 * Structure-first summary reads (Stage 3 milestone 3).
 *
 * Summary output is ADVISORY exploration, never authoritative source: it
 * states the exact revision it summarizes and is bounded so it can never
 * be larger than the source it represents. Semantic (model-generated)
 * summaries are explicitly out of scope for this milestone.
 */

export interface WorkspaceSummaryOptions {
  /** Hard cap on the summary text (UTF-8 bytes). */
  readonly maxBytes?: number;
  /** Number of notable function names to list. */
  readonly notableMethods?: number;
}

export interface WorkspaceSummaryResult {
  readonly path: string;
  readonly revision: WorkspaceRevisionHandle | null;
  readonly mode: "summary";
  /** Advisory overview text (bounded). */
  readonly text: string;
  /** Always true: a summary is never authoritative source. */
  readonly advisory: true;
  readonly truncated: boolean;
  readonly bytes: number;
}

export const DEFAULT_SUMMARY_MAX_BYTES = 4096;
export const DEFAULT_SUMMARY_NOTABLE_METHODS = 12;

function counted(entries: readonly unknown[], label: string): string {
  return entries.length === 0 ? "" : `${entries.length} ${label}`;
}

export function buildWorkspaceSummary(
  structure: GDScriptStructure,
  revision: WorkspaceRevisionHandle | null,
  options: WorkspaceSummaryOptions = {},
): WorkspaceSummaryResult {
  // The advisory footer and the truncation marker always fit: the summary
  // of a very small file may carry this constant overhead (documented),
  // and the byte accounting below reports the effective bound.
  const footerBytes =
    "\nadvisory structural summary \u2014 not authoritative source; read exact before editing."
      .length;
  const markerBytes = "\n\u2026 [summary truncated]".length;
  const maxBytes = Math.max(
    options.maxBytes ?? DEFAULT_SUMMARY_MAX_BYTES,
    footerBytes + markerBytes,
  );
  const notableMethods = options.notableMethods ?? DEFAULT_SUMMARY_NOTABLE_METHODS;
  const lines: string[] = [];
  const name = structure.path.split("/").pop() ?? structure.path;
  lines.push(`${name} (summary ${revision === null ? "no revision" : `@ ${revision}`})`);
  if (structure.extendsType !== null) {
    lines.push(`- extends ${structure.extendsType}`);
  }
  if (structure.className !== null) {
    lines.push(`- class_name ${structure.className}`);
  }
  if (structure.fileAnnotations.length > 0) {
    lines.push(
      `- annotations: ${structure.fileAnnotations.map((annotation) => `@${annotation.name}`).join(", ")}`,
    );
  }
  const counts = [
    counted(structure.signals, "signals"),
    counted(structure.properties, "properties"),
    counted(structure.functions, "functions"),
    counted(structure.constants, "constants"),
    counted(structure.enums, "enums"),
  ].filter((entry) => entry.length > 0);
  if (counts.length > 0) {
    lines.push(`- ${counts.join(", ")}`);
  }
  if (structure.properties.length > 0) {
    const exported = structure.properties.filter((property) =>
      property.annotations.some(
        (annotation) => annotation === "export" || annotation.startsWith("export_"),
      ),
    );
    if (exported.length > 0) {
      lines.push(`- exported properties: ${exported.map((property) => property.name).join(", ")}`);
    }
  }
  if (structure.functions.length > 0) {
    const notable = structure.functions
      .slice(0, notableMethods)
      .map((fn) => `${fn.name}${fn.isStatic ? " (static)" : ""}`)
      .join(", ");
    lines.push(
      `- functions: ${notable}${structure.functions.length > notableMethods ? `, ... (${structure.functions.length} total)` : ""}`,
    );
  }
  if (structure.dependencies.length > 0) {
    lines.push(`- dependencies: ${structure.dependencies.join(", ")}`);
  }
  if (structure.status === "partial") {
    lines.push(`- structural_status: partial (${structure.parserErrors.length} parser error(s))`);
  }
  if (structure.truncated) {
    lines.push("- structural output truncated (declaration cap reached)");
  }
  const encoder = new TextEncoder();
  // The advisory footer is never dropped: it is what stops a model from
  // mistaking a summary for authoritative source. The body is truncated
  // (byte-aware) to fit the budget together with the footer.
  const footer =
    "\nadvisory structural summary \u2014 not authoritative source; read exact before editing.";
  const body = lines.join("\n");
  let truncated = false;
  let bounded: string;
  if (encoder.encode(body + footer).length <= maxBytes) {
    bounded = body + footer;
  } else {
    const marker = "\n\u2026 [summary truncated]";
    let low = 0;
    let high = body.length;
    while (low < high) {
      const mid = Math.ceil((low + high) / 2);
      if (encoder.encode(body.slice(0, mid) + marker + footer).length <= maxBytes) {
        low = mid;
      } else {
        high = mid - 1;
      }
    }
    bounded = body.slice(0, low) + marker + footer;
    truncated = true;
  }
  return {
    path: structure.path,
    revision,
    mode: "summary",
    text: bounded,
    advisory: true,
    truncated,
    bytes: encoder.encode(bounded).length,
  };
}
