/**
 * Generic structural-document representation, normalization, and the
 * deterministic advisory summary formatter (Stage 3R R5, ADR 0016).
 *
 * A structural document carries only language-neutral facts: typed
 * declarations with optional names, opaque signature/detail text,
 * opaque attributes/modifiers, optional one-based lines, and nested
 * children; dependencies as opaque bounded facts; a complete/partial
 * status derived from typed issues; an explicit truncated flag; and
 * the R4 revision binding. The Host never interprets language
 * semantics: attribute strings such as `static` or `export` are data,
 * never meaning. A language implementation turns exact source at a
 * known revision into this bounded observation through its own
 * parser. The advisory summary formatter is the deterministic
 * language-neutral renderer (byte-bounded, revision-stating, always
 * advisory; the footer is never truncated away). A summary is
 * advisory and never authoritative source, and structural
 * information grants no read, write, capability, or completion
 * authority.
 *
 * The GDScript-specific scanner and summary (`workspace/gdscript-structure.ts`
 * and `workspace/workspace-summary.ts`) remain the Godot milestones'
 * reference and are intentionally NOT used here.
 */

/** Cross-language declaration categories (closed vocabulary). */
export type StructuralKind =
  | "type"
  | "function"
  | "method"
  | "field"
  | "variable"
  | "constant"
  | "enum"
  | "event"
  | "module"
  | "other";

/** The fixed generic kind order used by deterministic rendering. */
export const STRUCTURAL_KINDS: readonly StructuralKind[] = [
  "type",
  "function",
  "method",
  "field",
  "variable",
  "constant",
  "enum",
  "event",
  "module",
  "other",
] as const;

/** Parse a protocol kind string; unknown kinds are rejected. */
export function parseStructuralKind(value: unknown): StructuralKind | null {
  return STRUCTURAL_KINDS.includes(value as StructuralKind) ? (value as StructuralKind) : null;
}

/** One structural declaration (bounded, ordered, language-neutral). */
export interface StructuralDeclaration {
  readonly kind: StructuralKind;
  readonly name: string | null;
  readonly detail: string | null;
  /** One-based source line; null when unknown (never fabricated). */
  readonly line: number | null;
  /** Opaque attributes/modifiers; never interpreted by the Host. */
  readonly attributes: readonly string[];
  /** Nested declarations in document order (bounded). */
  readonly children: readonly StructuralDeclaration[];
}

/** One typed structural issue (parser/structure error). */
export interface StructuralIssue {
  /** One-based source line; null when unknown. */
  readonly line: number | null;
  /** Bounded issue message. */
  readonly message: string;
}

/** Bounds for structural normalization (mirrors the Rust core limits). */
export interface StructureOptions {
  readonly maxDeclarations: number;
  readonly maxDepth: number;
  readonly maxDependencies: number;
  readonly maxIssues: number;
}

/** Default structural bounds, identical to the Rust core `LANGUAGE_LIMITS`. */
export const DEFAULT_STRUCTURE_LIMITS: StructureOptions = {
  maxDeclarations: 256,
  maxDepth: 16,
  maxDependencies: 32,
  maxIssues: 64,
};

/** Structure parse status: complete (no issues) or partial (issues). */
export type StructureStatus = "complete" | "partial";

/** A normalized, bounded structural document for one exact source state. */
export interface StructuralDocument {
  /** Workspace-relative path with `/` separators. */
  readonly path: string;
  /** R4 revision handle of the exact source state observed. */
  readonly revision: string | null;
  /** Declarations in document order (depth-first, bounded). */
  readonly declarations: readonly StructuralDeclaration[];
  /** Bounded opaque dependencies. */
  readonly dependencies: readonly string[];
  /** Parse status derived from the bounded issues. */
  readonly status: StructureStatus;
  /** Typed issues (source order, bounded). */
  readonly issues: readonly StructuralIssue[];
  /** True when any output bound was applied (explicit truncation). */
  readonly truncated: boolean;
}

/** Total declaration count across the whole bounded tree. */
export function declarationCount(document: StructuralDocument): number {
  let count = 0;
  const walk = (declarations: readonly StructuralDeclaration[]): void => {
    for (const declaration of declarations) {
      count += 1;
      walk(declaration.children);
    }
  };
  walk(document.declarations);
  return count;
}

/**
 * Normalize one structural observation: preserve document order, bound
 * the declaration tree (count and depth), dependencies, and issues with
 * explicit truncation, and derive the status from the bounded issues.
 */
export function normalizeStructuralDocument(
  path: string,
  declarations: readonly StructuralDeclaration[],
  dependencies: readonly string[],
  issues: readonly StructuralIssue[],
  options: StructureOptions = DEFAULT_STRUCTURE_LIMITS,
): StructuralDocument {
  const state = { truncated: false, budget: options.maxDeclarations };
  const bounded = boundDeclarations(declarations, options.maxDepth, state);
  const boundedDependencies =
    dependencies.length > options.maxDependencies
      ? dependencies.slice(0, options.maxDependencies)
      : dependencies;
  if (dependencies.length > options.maxDependencies) {
    state.truncated = true;
  }
  const boundedIssues =
    issues.length > options.maxIssues ? issues.slice(0, options.maxIssues) : issues;
  if (issues.length > options.maxIssues) {
    state.truncated = true;
  }
  return {
    path,
    revision: null,
    declarations: bounded,
    dependencies: boundedDependencies,
    status: boundedIssues.length === 0 ? "complete" : "partial",
    issues: boundedIssues,
    truncated: state.truncated,
  };
}

interface BoundState {
  truncated: boolean;
  budget: number;
}

/**
 * Depth-first bounded walk of the declaration tree in document order.
 * Recursion is bounded by `maxDepth` (never by the input shape):
 * subtrees deeper than the bound are excluded without recursing into
 * them.
 */
function boundDeclarations(
  declarations: readonly StructuralDeclaration[],
  maxDepth: number,
  state: BoundState,
): readonly StructuralDeclaration[] {
  const walk = (
    nodes: readonly StructuralDeclaration[],
    depth: number,
  ): readonly StructuralDeclaration[] => {
    const out: StructuralDeclaration[] = [];
    for (const node of nodes) {
      if (state.budget === 0) {
        state.truncated = true;
        break;
      }
      if (depth > maxDepth) {
        state.truncated = true;
        continue;
      }
      state.budget -= 1;
      const children = walk(node.children, depth + 1);
      out.push({ ...node, children });
    }
    return out;
  };
  return walk(declarations, 1);
}

/** Summary formatting options. */
export interface SummaryOptions {
  /** Hard cap on the summary text (UTF-8 bytes). */
  readonly maxBytes?: number;
  /** Number of notable top-level declaration names to list. */
  readonly notableDeclarations?: number;
}

/** Default advisory summary byte budget (mirrors the Rust core). */
export const DEFAULT_SUMMARY_MAX_BYTES = 4096;

/** Default number of notable top-level declaration names (mirrors the Rust core). */
export const DEFAULT_SUMMARY_NOTABLE_DECLARATIONS = 12;

/** The advisory footer; it is never truncated away. */
export const SUMMARY_FOOTER =
  "\nadvisory structural summary \u2014 not authoritative source; read exact before editing.";

/** The explicit truncation marker. */
export const SUMMARY_TRUNCATION_MARKER = "\n\u2026 [summary truncated]";

/** A rendered advisory structural summary (bounded, never authoritative). */
export interface StructuralSummary {
  readonly path: string;
  /** R4 revision handle the summary describes; null when unknown. */
  readonly revision: string | null;
  /** Bounded advisory text (footer always present). */
  readonly text: string;
  /** True when the body was byte-truncated to fit the budget. */
  readonly truncated: boolean;
  /** UTF-8 byte length of the returned text. */
  readonly bytes: number;
}

/** Build the deterministic advisory structural summary (language-neutral). */
export function buildStructuralSummary(
  document: StructuralDocument,
  options: SummaryOptions = {},
): StructuralSummary {
  // The advisory footer and the truncation marker always fit: the
  // summary of a very small file may carry this constant overhead
  // (documented), and the byte accounting below reports the effective
  // bound.
  const footerBytes = SUMMARY_FOOTER.length;
  const markerBytes = SUMMARY_TRUNCATION_MARKER.length;
  const maxBytes = Math.max(
    options.maxBytes ?? DEFAULT_SUMMARY_MAX_BYTES,
    footerBytes + markerBytes,
  );
  const notable = options.notableDeclarations ?? DEFAULT_SUMMARY_NOTABLE_DECLARATIONS;
  const lines: string[] = [];
  const name = document.path.split("/").pop() ?? document.path;
  lines.push(
    document.revision === null
      ? `${name} (summary no revision)`
      : `${name} (summary @ ${document.revision})`,
  );
  const total = declarationCount(document);
  if (total > 0) {
    const counts = STRUCTURAL_KINDS.map((kind) => ({
      kind,
      count: countKind(document.declarations, kind),
    }))
      .filter((entry) => entry.count > 0)
      .map((entry) => `${entry.kind}: ${entry.count}`)
      .join(", ");
    lines.push(`- declarations: ${total} (${counts})`);
  }
  const namedTopLevel: string[] = [];
  for (const declaration of document.declarations) {
    if (declaration.name !== null) {
      namedTopLevel.push(declaration.name);
    }
  }
  if (namedTopLevel.length > 0) {
    const listed = namedTopLevel.slice(0, notable).join(", ");
    lines.push(
      namedTopLevel.length > notable
        ? `- top-level: ${listed}, ... (${namedTopLevel.length} total)`
        : `- top-level: ${listed}`,
    );
  }
  if (document.dependencies.length > 0) {
    lines.push(`- dependencies: ${document.dependencies.join(", ")}`);
  }
  if (document.status === "partial") {
    lines.push(`- structural status: partial (${document.issues.length} issue(s))`);
  }
  if (document.truncated) {
    lines.push("- structural output truncated (output bound reached)");
  }
  const encoder = new TextEncoder();
  // The advisory footer is never dropped: the body is truncated
  // (byte-aware, UTF-16-unit slicing like the reference) to fit the
  // budget together with the footer.
  const body = lines.join("\n");
  let truncated = false;
  let bounded = "";
  if (encoder.encode(body + SUMMARY_FOOTER).length <= maxBytes) {
    bounded = body + SUMMARY_FOOTER;
  } else {
    const marker = SUMMARY_TRUNCATION_MARKER;
    let low = 0;
    let high = body.length;
    while (low < high) {
      const mid = Math.ceil((low + high) / 2);
      if (encoder.encode(body.slice(0, mid) + marker + SUMMARY_FOOTER).length <= maxBytes) {
        low = mid;
      } else {
        high = mid - 1;
      }
    }
    bounded = body.slice(0, low) + marker + SUMMARY_FOOTER;
    truncated = true;
  }
  return {
    path: document.path,
    revision: document.revision,
    text: bounded,
    truncated,
    bytes: encoder.encode(bounded).length,
  };
}

function countKind(declarations: readonly StructuralDeclaration[], kind: StructuralKind): number {
  let count = 0;
  for (const declaration of declarations) {
    if (declaration.kind === kind) {
      count += 1;
    }
    count += countKind(declaration.children, kind);
  }
  return count;
}
