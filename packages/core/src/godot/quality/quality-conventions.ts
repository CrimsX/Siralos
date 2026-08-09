import { QUALITY_LIMITS } from "./quality-model.js";

/**
 * Small read-only GDScript convention analyzer (§14–§16).
 *
 * Not a full GDScript linter: only high-confidence issues in NEW or
 * MODIFIED lines of the change are reported. The analyzer never rewrites
 * code and never enforces the official Godot style guide over an
 * established project style. Priority: explicit repository guidance
 * (only when provided as deterministic mandatory rules), then existing
 * local file/module conventions inferred from the file itself, then
 * project-wide conventions, then Godot style recommendations as fallback.
 * All findings are advisory unless a rule is explicitly marked
 * repository-mandatory, in which case it is a `warning` (§16).
 */

export type ConventionRule =
  | "trailing-whitespace"
  | "mixed-indentation"
  | "long-line"
  | "multiple-statements"
  | "naming-drift"
  | "indentation-width-mismatch"
  | "typed-signature-drift";

export type ConventionBasis =
  "repository-guidance" | "local-convention" | "project-convention" | "godot-fallback";

export interface ConventionFinding {
  readonly severity: "advisory" | "warning";
  readonly rule: ConventionRule;
  readonly path: string;
  readonly line: number | null;
  readonly message: string;
  readonly basis: ConventionBasis;
}

/** One changed file of the final change set, with its post-change content. */
export interface ConventionChangeInput {
  readonly path: string;
  readonly operation: "create" | "update" | "delete";
  /** Complete post-change content (null for delete). */
  readonly afterContent: string | null;
  /** Deterministic bounded unified diff (empty for delete). */
  readonly unifiedDiff: string;
}

export interface ConventionAnalysisOptions {
  /**
   * Deterministic repository-guidance hooks: rules marked mandatory here
   * are reported with `warning` severity and may block completion. Empty
   * by default — no convention blocks unless the repository explicitly
   * requires it. This is not a natural-language policy engine.
   */
  readonly mandatoryRules?: readonly ConventionRule[];
  readonly longLineChars?: number;
}

interface AddedLine {
  readonly text: string;
  readonly line: number;
}

const DECLARATION_PATTERN = /\b(?:func|var|const|static var)\s+([A-Za-z_][A-Za-z0-9_]*)\b/;

/**
 * Deterministic convention analysis over the changed lines of a change
 * set (§14). Operates on line text and the post-change file content only;
 * no semantic parsing, no regex-driven type inference beyond the
 * high-confidence checks below.
 */
export function analyzeConventions(
  changes: readonly ConventionChangeInput[],
  options: ConventionAnalysisOptions = {},
): readonly ConventionFinding[] {
  const findings: ConventionFinding[] = [];
  const longLineChars = options.longLineChars ?? QUALITY_LIMITS.longLineChars;
  const mandatory = new Set(options.mandatoryRules ?? []);
  const severityFor = (rule: ConventionRule): "advisory" | "warning" =>
    mandatory.has(rule) ? "warning" : "advisory";
  const basisFor = (rule: ConventionRule): ConventionBasis =>
    mandatory.has(rule) ? "repository-guidance" : "local-convention";

  for (const change of changes) {
    if (change.operation === "delete" || change.afterContent === null) {
      continue;
    }
    const added = extractAddedLines(change.unifiedDiff);
    const fileLines = change.afterContent.split("\n");

    // Mixed indentation: both tabs and spaces at line start among the
    // newly added lines.
    const tabIndented = added.filter((line) => line.text.startsWith("\t"));
    const spaceIndented = added.filter((line) => /^ +/.test(line.text));
    if (tabIndented.length > 0 && spaceIndented.length > 0) {
      findings.push({
        severity: severityFor("mixed-indentation"),
        rule: "mixed-indentation",
        path: change.path,
        line: null,
        message:
          "The change mixes tab and space indentation; use one indentation style for the block.",
        basis: basisFor("mixed-indentation"),
      });
    }

    const fileIndent = dominantIndentUnit(fileLines);
    for (const line of added) {
      // Trailing whitespace on a changed line.
      if (/[ \t]+$/.test(line.text)) {
        findings.push({
          severity: severityFor("trailing-whitespace"),
          rule: "trailing-whitespace",
          path: change.path,
          line: line.line,
          message: "Trailing whitespace on a newly introduced line.",
          basis: basisFor("trailing-whitespace"),
        });
      }
      // Very long newly introduced lines.
      if (line.text.length > longLineChars) {
        findings.push({
          severity: severityFor("long-line"),
          rule: "long-line",
          path: change.path,
          line: line.line,
          message: `Newly introduced line exceeds ${longLineChars} characters.`,
          basis: basisFor("long-line"),
        });
      }
      // Multiple statements on one newly introduced line: a statement
      // separator followed by more code. String literals are stripped so
      // semicolons inside strings are never counted.
      const trimmed = line.text.trim();
      const withoutStrings = trimmed.replace(/"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'/g, "");
      if (/;\s*[A-Za-z_]/.test(withoutStrings)) {
        findings.push({
          severity: severityFor("multiple-statements"),
          rule: "multiple-statements",
          path: change.path,
          line: line.line,
          message: "Multiple statements appear on one newly introduced line.",
          basis: basisFor("multiple-statements"),
        });
      }
      // Indentation width mismatch with the file's dominant unit.
      if (fileIndent !== null && line.text.length > 0) {
        const lineIndent = indentUnitOf(line.text);
        if (lineIndent !== null && lineIndent !== fileIndent) {
          findings.push({
            severity: severityFor("indentation-width-mismatch"),
            rule: "indentation-width-mismatch",
            path: change.path,
            line: line.line,
            message: `The change indents with ${describeIndent(lineIndent)} while the file uses ${describeIndent(fileIndent)}.`,
            basis: basisFor("indentation-width-mismatch"),
          });
        }
      }
    }

    analyzeNamingDrift(change, added, findings, severityFor);
    analyzeTypedSignatureDrift(change, added, fileLines, findings, severityFor);
  }

  return findings.slice(0, QUALITY_LIMITS.maxConventionFindings);
}

/**
 * Extracts added lines with their absolute line numbers from a standard
 * unified diff (`@@ -start,count +start,count @@` hunks). Lines outside
 * hunks (headers) are ignored; a line whose absolute number cannot be
 * determined is reported with `null`.
 */
export function extractAddedLines(unifiedDiff: string): readonly AddedLine[] {
  const lines = unifiedDiff.split("\n");
  const added: AddedLine[] = [];
  let afterLine = 0;
  for (const raw of lines) {
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(raw);
    if (hunk !== null) {
      afterLine = Number.parseInt(hunk[2] ?? "0", 10);
      continue;
    }
    if (raw.startsWith("+++") || raw.startsWith("---") || raw.startsWith("Index:")) {
      continue;
    }
    if (raw.startsWith("+")) {
      added.push({ text: raw.slice(1), line: afterLine });
      afterLine += 1;
      continue;
    }
    if (raw.startsWith("-")) {
      continue;
    }
    if (raw.startsWith(" ")) {
      afterLine += 1;
      continue;
    }
    if (raw.startsWith("\\ No newline")) {
      continue;
    }
  }
  return added;
}

/** Dominant indentation unit of an existing file (tabs, 2 or 4 spaces, or null). */
function dominantIndentUnit(lines: readonly string[]): string | null {
  const counts = new Map<string, number>();
  for (const line of lines) {
    const unit = indentUnitOf(line);
    if (unit !== null) {
      counts.set(unit, (counts.get(unit) ?? 0) + 1);
    }
  }
  let best: string | null = null;
  let bestCount = 0;
  for (const [unit, count] of counts) {
    if (count > bestCount) {
      best = unit;
      bestCount = count;
    }
  }
  return bestCount >= 2 ? best : null;
}

function indentUnitOf(line: string): string | null {
  const leading = line.match(/^[ \t]+/)?.[0];
  if (leading === undefined || leading.length === 0) {
    return null;
  }
  if (leading.startsWith("\t")) {
    return "\t";
  }
  const width = leading.length;
  if (width % 4 === 0) {
    return "    ";
  }
  if (width % 2 === 0) {
    return "  ";
  }
  return null;
}

function describeIndent(unit: string): string {
  if (unit === "\t") {
    return "tabs";
  }
  return `${unit.length} spaces`;
}

/**
 * Naming-convention drift on newly declared identifiers. The file's
 * EXISTING declarations (excluding the changed lines) are the local
 * convention; a change that adds identifiers contradicting the dominant
 * local style is flagged only when the file has enough existing
 * declarations to infer a convention.
 */
function analyzeNamingDrift(
  change: ConventionChangeInput,
  added: readonly AddedLine[],
  findings: ConventionFinding[],
  severityFor: (rule: ConventionRule) => "advisory" | "warning",
): void {
  const existing = collectDeclarationNamesExcluding(change.afterContent ?? "", added);
  const localStyle = dominantStyle(existing);
  if (localStyle === null) {
    return;
  }
  for (const line of added) {
    const match = DECLARATION_PATTERN.exec(line.text.trim());
    if (match === null) {
      continue;
    }
    const name = match[1];
    if (name === undefined) {
      continue;
    }
    if (isSnakeCase(name) !== (localStyle === "snake")) {
      findings.push({
        severity: severityFor("naming-drift"),
        rule: "naming-drift",
        path: change.path,
        line: line.line,
        message: `Newly declared "${name}" contradicts the file's dominant ${localStyle === "snake" ? "snake_case" : "camelCase"} convention.`,
        basis: "local-convention",
      });
    }
  }
}

function collectDeclarationNamesExcluding(
  content: string,
  added: readonly AddedLine[],
): readonly string[] {
  const addedLines = new Set(added.map((line) => line.line));
  const names: string[] = [];
  const lines = content.split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    if (addedLines.has(index + 1)) {
      continue;
    }
    const match = DECLARATION_PATTERN.exec(lines[index] ?? "");
    if (match !== null && match[1] !== undefined) {
      names.push(match[1]);
    }
  }
  return names;
}

function dominantStyle(names: readonly string[]): "snake" | "camel" | null {
  if (names.length < 3) {
    return null;
  }
  const snake = names.filter(isSnakeCase).length;
  const camel = names.length - snake;
  if (snake >= names.length * 0.6) {
    return "snake";
  }
  if (camel >= names.length * 0.6) {
    return "camel";
  }
  return null;
}

/**
 * Case classification for GDScript identifiers. A single leading
 * underscore (the Godot callback modifier, e.g. `_ready`,
 * `_physics_process`) does not change the case of the name and is
 * stripped before testing.
 */
function isSnakeCase(name: string): boolean {
  const withoutModifier = name.startsWith("_") ? name.slice(1) : name;
  return /^[a-z0-9]+(?:_[a-z0-9]+)*$/.test(withoutModifier);
}

/**
 * Typed-file style preservation (§15). When the file's existing functions
 * are predominantly type-annotated and the change adds an untyped
 * function, a style advisory is returned; a predominantly dynamic file is
 * never forced to add annotations, and mixed/uncertain files get nothing.
 */
function analyzeTypedSignatureDrift(
  change: ConventionChangeInput,
  added: readonly AddedLine[],
  fileLines: readonly string[],
  findings: ConventionFinding[],
  severityFor: (rule: ConventionRule) => "advisory" | "warning",
): void {
  const existing = fileLines.filter((line) => /^\s*func\s+/.test(line));
  if (existing.length < 2) {
    return;
  }
  const typed = existing.filter((line) => /->/.test(line) || /:\s*\S+\s*\)/.test(line)).length;
  const isTypedFile = typed / existing.length >= 0.6;
  if (!isTypedFile) {
    return;
  }
  for (const line of added) {
    if (/^\s*func\s+/.test(line.text) && !/->/.test(line.text)) {
      findings.push({
        severity: severityFor("typed-signature-drift"),
        rule: "typed-signature-drift",
        path: change.path,
        line: line.line,
        message:
          "The file's existing functions are predominantly type-annotated; the new function omits the return type.",
        basis: "local-convention",
      });
    }
  }
}
