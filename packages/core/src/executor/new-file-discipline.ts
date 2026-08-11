import { deepFreeze } from "../domain/deep-freeze.js";

/**
 * New-file discipline and scope evaluation (harness context optimization,
 * ADR 0023; milestone Parts I and J).
 *
 * The architecture preference is: extend the existing module owner for a
 * responsibility before creating another adjacent abstraction. New
 * production files therefore carry a bounded host-visible rationale that
 * names the existing owners inspected. This is architecture/scope
 * evidence, not an approval gate.
 *
 * Proliferation signals are DETERMINISTIC REVIEW SIGNALS, not hard rules:
 * heuristics flag suspicious expansion (many new files, new directories
 * for a narrow change, tiny one-use helpers, many files outside the
 * planned scope) and feed existing quality/review findings. They never
 * block legitimate discovery — scope is not a prison; expansion with
 * evidence is recorded and continues.
 */

export interface NewFileRationale {
  /** Workspace-relative path of the new production file. */
  readonly path: string;
  /** Why a new file (and not an existing owner) is justified. */
  readonly reason: string;
  /** Existing owner modules inspected before creating the file. */
  readonly existingOwnersInspected: readonly string[];
}

export interface ProliferationSignal {
  /** Stable signal id (e.g. PROLIF.MANY_NEW_FILES). */
  readonly id: string;
  readonly message: string;
}

export type ScopeDiffClassification = "expected" | "justified expansion" | "unexplained expansion";

export interface ScopeDiffEntry {
  readonly path: string;
  readonly classification: ScopeDiffClassification;
  /** Present for justified expansions: the recorded rationale. */
  readonly rationale?: string;
}

export interface ScopeDiffReport {
  readonly entries: readonly ScopeDiffEntry[];
  readonly unexplained: readonly string[];
}

/** Host-owned hard bounds (never raised by input). */
export const NEW_FILE_DISCIPLINE_LIMITS = Object.freeze({
  maxRationales: 64,
  maxPathBytes: 1024,
  maxReasonBytes: 1024,
  maxOwners: 8,
  maxOwnerBytes: 256,
  maxSignals: 32,
  maxSignalMessageBytes: 512,
  maxDiffEntries: 256,
});

/**
 * Deterministic proliferation heuristics. Thresholds are review signals,
 * not correctness rules: they flag suspicious expansion for the reviewer;
 * a legitimate change may exceed them with evidence.
 */
export const PROLIFERATION_HEURISTICS = Object.freeze({
  /** More than this many new production files in one task is suspicious. */
  maxNewProductionFiles: 5,
  /** More than this many tiny helper files (below the byte floor) is suspicious. */
  maxTinyHelperFiles: 2,
  /** New files whose size is below this many bytes are considered tiny. */
  tinyFileBytes: 256,
  /** More than this many changed files outside the planned scope is suspicious. */
  maxChangedOutsideScope: 3,
});

const textEncoder = new TextEncoder();

function validatePath(path: string): string {
  const text = path.trim();
  if (text.length === 0) {
    throw new Error("A file path must not be empty.");
  }
  if (textEncoder.encode(text).length > NEW_FILE_DISCIPLINE_LIMITS.maxPathBytes) {
    throw new Error(`A file path exceeds ${NEW_FILE_DISCIPLINE_LIMITS.maxPathBytes} UTF-8 bytes.`);
  }
  return text;
}

function validateReason(reason: string): string {
  const text = reason.trim();
  if (text.length === 0) {
    throw new Error("A new-file rationale requires a reason.");
  }
  if (textEncoder.encode(text).length > NEW_FILE_DISCIPLINE_LIMITS.maxReasonBytes) {
    throw new Error(
      `A new-file rationale reason exceeds ${NEW_FILE_DISCIPLINE_LIMITS.maxReasonBytes} UTF-8 bytes.`,
    );
  }
  return text;
}

function validateOwners(owners: readonly string[]): string[] {
  if (owners.length > NEW_FILE_DISCIPLINE_LIMITS.maxOwners) {
    throw new Error(
      `A new-file rationale names at most ${NEW_FILE_DISCIPLINE_LIMITS.maxOwners} owners.`,
    );
  }
  return owners.map((owner) => {
    const text = owner.trim();
    if (text.length === 0) {
      throw new Error("An inspected-owner name must not be empty.");
    }
    if (textEncoder.encode(text).length > NEW_FILE_DISCIPLINE_LIMITS.maxOwnerBytes) {
      throw new Error(
        `An inspected-owner name exceeds ${NEW_FILE_DISCIPLINE_LIMITS.maxOwnerBytes} UTF-8 bytes.`,
      );
    }
    return text;
  });
}

/**
 * Record the host-visible rationale for a new production file. Validated,
 * bounded, deterministic; an empty reason is rejected (a new file with no
 * justification is exactly what the discipline prevents).
 */
export function createNewFileRationale(input: NewFileRationale): NewFileRationale {
  return deepFreeze({
    path: validatePath(input.path),
    reason: validateReason(input.reason),
    existingOwnersInspected: validateOwners(input.existingOwnersInspected),
  });
}

function directoryOf(path: string): string {
  const index = path.lastIndexOf("/");
  return index < 0 ? "." : path.slice(0, index);
}

/**
 * Deterministic proliferation signals over host-observed facts (new file
 * paths + sizes, planned scope paths, known directories). These are
 * review signals: they never block work and never become policy.
 */
export function detectProliferationSignals(input: {
  readonly newProductionFiles: readonly { readonly path: string; readonly sizeBytes: number }[];
  /** Planned/expected workspace-relative paths (plan touchpoints + verified files). */
  readonly plannedPaths: readonly string[];
  /** Workspace-relative directories known to already exist. */
  readonly knownDirectories: readonly string[];
}): readonly ProliferationSignal[] {
  const signals: ProliferationSignal[] = [];
  const files = input.newProductionFiles;
  if (files.length > PROLIFERATION_HEURISTICS.maxNewProductionFiles) {
    signals.push({
      id: "PROLIF.MANY_NEW_FILES",
      message: `${files.length} new production files in one task exceeds the ${PROLIFERATION_HEURISTICS.maxNewProductionFiles}-file review signal; confirm each has a recorded rationale.`,
    });
  }
  const tiny = files.filter((file) => file.sizeBytes < PROLIFERATION_HEURISTICS.tinyFileBytes);
  if (tiny.length > PROLIFERATION_HEURISTICS.maxTinyHelperFiles) {
    signals.push({
      id: "PROLIF.TINY_HELPERS",
      message: `${tiny.length} tiny helper files (under ${PROLIFERATION_HEURISTICS.tinyFileBytes} bytes); consider extending an existing owner.`,
    });
  }
  const newDirectories = [...new Set(files.map((file) => directoryOf(file.path)))].filter(
    (directory) => directory !== "." && !input.knownDirectories.includes(directory),
  );
  if (newDirectories.length > 0) {
    signals.push({
      id: "PROLIF.NEW_DIRECTORY",
      message: `new directories created for this change: ${newDirectories.join(", ")}; verify a distinct responsibility boundary exists.`,
    });
  }
  const outsideScope = files.filter(
    (file) => !input.plannedPaths.some((planned) => pathMatchesPattern(file.path, planned)),
  );
  if (outsideScope.length > PROLIFERATION_HEURISTICS.maxChangedOutsideScope) {
    signals.push({
      id: "PROLIF.OUTSIDE_SCOPE",
      message: `${outsideScope.length} new files do not match any planned path; record evidence and promote through the scope before treating them as expected.`,
    });
  }
  return signals.slice(0, NEW_FILE_DISCIPLINE_LIMITS.maxSignals).map((signal) => ({
    id: signal.id,
    message:
      textEncoder.encode(signal.message).length > NEW_FILE_DISCIPLINE_LIMITS.maxSignalMessageBytes
        ? `${signal.message.slice(0, 240)}\u2026`
        : signal.message,
  }));
}

/**
 * Minimal deterministic glob support for workspace paths: `*` matches
 * within one path segment, `**` matches across segments. A pattern with
 * no wildcards matches the exact path only.
 */
export function pathMatchesPattern(path: string, pattern: string): boolean {
  if (!pattern.includes("*")) {
    return path === pattern;
  }
  const segments = pattern.split("/");
  const regexParts: string[] = [];
  for (const segment of segments) {
    if (segment === "**") {
      // `**` matches zero or more whole path segments plus an optional
      // final segment (so a trailing `**` also covers the file itself).
      regexParts.push("(?:[^/]+/)*[^/]*");
    } else if (segment.includes("*")) {
      const escaped = segment.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, "[^/]*");
      regexParts.push(escaped);
    } else {
      regexParts.push(segment.replace(/[.+^${}()|[\]\\]/g, "\\$&"));
    }
  }
  return new RegExp(`^${regexParts.join("/")}$`).test(path);
}

/**
 * Compare the planned scope with the actual changed files at completion.
 * Files matching a planned path are expected; files with a recorded
 * rationale are justified expansion; everything else is unexplained
 * expansion and feeds existing quality/review findings.
 */
export function evaluateScopeDiff(input: {
  /** Planned/expected workspace-relative paths (touchpoints, globs allowed). */
  readonly plannedPaths: readonly string[];
  /** Actual changed files (workspace-relative). */
  readonly changedPaths: readonly string[];
  /** Rationales recorded during execution (justified expansion). */
  readonly rationales: readonly NewFileRationale[];
}): ScopeDiffReport {
  if (input.changedPaths.length > NEW_FILE_DISCIPLINE_LIMITS.maxDiffEntries) {
    throw new Error(
      `Scope evaluation accepts at most ${NEW_FILE_DISCIPLINE_LIMITS.maxDiffEntries} changed paths.`,
    );
  }
  const rationaleByPath = new Map<string, string>();
  for (const rationale of input.rationales) {
    rationaleByPath.set(rationale.path, rationale.reason);
  }
  const entries: ScopeDiffEntry[] = [];
  const unexplained: string[] = [];
  for (const path of input.changedPaths) {
    const normalized = validatePath(path);
    if (input.plannedPaths.some((planned) => pathMatchesPattern(normalized, planned))) {
      entries.push({ path: normalized, classification: "expected" });
    } else {
      const rationale = rationaleByPath.get(normalized);
      if (rationale !== undefined) {
        entries.push({ path: normalized, classification: "justified expansion", rationale });
      } else {
        entries.push({ path: normalized, classification: "unexplained expansion" });
        unexplained.push(normalized);
      }
    }
  }
  return deepFreeze({ entries, unexplained });
}
