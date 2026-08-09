import type { GodotGDScriptDiagnostic } from "../gdscript.js";
import { QUALITY_LIMITS } from "./quality-model.js";

/**
 * Warning baseline and delta (§10–§11).
 *
 * Before applying a development change, bounded diagnostics for the
 * relevant files are recorded where practical (the pre-edit language
 * session). After the change the before/after diagnostics are compared
 * with stable normalized identities — never raw message ordering — and
 * every diagnostic is classified `introduced`, `resolved`, `unchanged`,
 * or `uncertain`. Attribution is conservative: when identity cannot be
 * proven (line movement beyond tolerance, unavailable baseline), the
 * entry is labelled uncertain rather than falsely attributed.
 */

export type WarningClassification = "introduced" | "resolved" | "unchanged" | "uncertain";

export interface WarningDeltaEntry {
  readonly path: string;
  readonly line: number | null;
  readonly code: string | null;
  readonly message: string;
  readonly severity: "error" | "warning" | "info" | "unknown";
  readonly classification: WarningClassification;
}

export interface WarningDeltaSummary {
  /** False when no baseline could be captured (infrastructure unavailable). */
  readonly baselineAvailable: boolean;
  readonly introducedErrors: number;
  readonly introducedWarnings: number;
  readonly resolvedWarnings: number;
  readonly unchangedWarnings: number;
  readonly uncertainWarnings: number;
  readonly entries: readonly WarningDeltaEntry[];
}

/**
 * Stable identity over a diagnostic: path + code + normalized message.
 * Normalization replaces standalone integers (line/column numbers embedded
 * in messages) with `#` and collapses whitespace, so the identity does not
 * depend on exact line numbers or message formatting.
 */
export function diagnosticIdentityKey(diagnostic: {
  readonly path: string | null;
  readonly code: string | null;
  readonly message: string;
}): string {
  return [
    diagnostic.path ?? "",
    diagnostic.code ?? "",
    normalizeDiagnosticMessage(diagnostic.message),
  ].join("\u0000");
}

/** Bounded normalization: integers become `#`, whitespace collapses. */
export function normalizeDiagnosticMessage(message: string): string {
  return message
    .replace(/\b\d+\b/g, "#")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 2048);
}

/**
 * Deterministic warning delta (§11). `baseline` is the pre-edit
 * diagnostics for the changed files (empty when capture was not possible —
 * the caller passes `baselineAvailable: false`), `after` is the post-edit
 * diagnostics. Matching is by stable identity per file; within one
 * identity, after-entries are matched to the closest unused baseline line.
 * A matched identity whose line moved beyond the tolerance is `uncertain`;
 * an after-entry without a baseline match is `introduced`; a baseline
 * entry without an after match is `resolved`.
 */
export function computeWarningDelta(
  baseline: readonly GodotGDScriptDiagnostic[],
  after: readonly GodotGDScriptDiagnostic[],
  changedPaths: readonly string[],
  options: { readonly baselineAvailable?: boolean; readonly lineTolerance?: number } = {},
): WarningDeltaSummary {
  const tolerance = options.lineTolerance ?? QUALITY_LIMITS.warningLineTolerance;
  const changed = new Set(changedPaths);
  const baselineEntries = baseline.filter(
    (entry) => entry.path !== null && changed.has(entry.path),
  );
  const afterEntries = after.filter((entry) => entry.path !== null && changed.has(entry.path));

  const baselineByKey = new Map<string, GodotGDScriptDiagnostic[]>();
  for (const entry of baselineEntries) {
    const key = diagnosticIdentityKey(entry);
    const list = baselineByKey.get(key);
    if (list === undefined) {
      baselineByKey.set(key, [entry]);
    } else {
      list.push(entry);
    }
  }
  for (const list of baselineByKey.values()) {
    list.sort((left, right) => (left.line ?? 0) - (right.line ?? 0));
  }

  const entries: WarningDeltaEntry[] = [];
  const afterByKey = new Map<string, GodotGDScriptDiagnostic[]>();
  for (const entry of afterEntries) {
    const key = diagnosticIdentityKey(entry);
    const list = afterByKey.get(key);
    if (list === undefined) {
      afterByKey.set(key, [entry]);
    } else {
      list.push(entry);
    }
  }
  for (const list of afterByKey.values()) {
    list.sort((left, right) => (left.line ?? 0) - (right.line ?? 0));
  }

  const usedBaselineByKey = new Map<string, Set<number>>();
  for (const [key, afterList] of afterByKey) {
    const baselineList = baselineByKey.get(key) ?? [];
    const used = new Set<number>();
    for (const entry of afterList) {
      let classification: WarningClassification;
      if (baselineList.length === 0) {
        classification = "introduced";
      } else {
        let bestIndex = -1;
        let bestDelta = Infinity;
        for (let index = 0; index < baselineList.length; index += 1) {
          if (used.has(index)) {
            continue;
          }
          const delta = Math.abs((entry.line ?? 0) - (baselineList[index]?.line ?? 0));
          if (delta < bestDelta) {
            bestDelta = delta;
            bestIndex = index;
          }
        }
        if (bestIndex >= 0) {
          used.add(bestIndex);
          classification = bestDelta <= tolerance ? "unchanged" : "uncertain";
        } else {
          classification = "uncertain";
        }
      }
      entries.push({
        path: entry.path ?? "",
        line: entry.line,
        code: entry.code,
        message: entry.message,
        severity: entry.severity,
        classification,
      });
    }
    usedBaselineByKey.set(key, used);
  }

  // Resolved: baseline entries that no after-entry matched, per identity.
  // An identity with more baseline instances than after instances has its
  // unmatched baseline instances reported resolved — never dropped.
  for (const [key, baselineList] of baselineByKey) {
    const used = usedBaselineByKey.get(key) ?? new Set<number>();
    for (let index = 0; index < baselineList.length; index += 1) {
      if (used.has(index)) {
        continue;
      }
      const entry = baselineList[index];
      if (entry === undefined) {
        continue;
      }
      entries.push({
        path: entry.path ?? "",
        line: entry.line,
        code: entry.code,
        message: entry.message,
        severity: entry.severity,
        classification: "resolved",
      });
    }
  }

  entries.sort((left, right) => {
    if (left.path !== right.path) {
      return left.path < right.path ? -1 : 1;
    }
    const leftLine = left.line ?? -1;
    const rightLine = right.line ?? -1;
    if (leftLine !== rightLine) {
      return leftLine - rightLine;
    }
    if (left.message !== right.message) {
      return left.message < right.message ? -1 : 1;
    }
    return 0;
  });

  const bounded = entries.slice(0, QUALITY_LIMITS.maxWarningDeltaEntries);
  return {
    baselineAvailable: options.baselineAvailable ?? baseline.length > 0,
    introducedErrors: bounded.filter(
      (entry) => entry.classification === "introduced" && entry.severity === "error",
    ).length,
    introducedWarnings: bounded.filter(
      (entry) => entry.classification === "introduced" && entry.severity === "warning",
    ).length,
    resolvedWarnings: bounded.filter(
      (entry) => entry.classification === "resolved" && entry.severity === "warning",
    ).length,
    unchangedWarnings: bounded.filter(
      (entry) => entry.classification === "unchanged" && entry.severity === "warning",
    ).length,
    uncertainWarnings: bounded.filter(
      (entry) => entry.classification === "uncertain" && entry.severity === "warning",
    ).length,
    entries: bounded,
  };
}
