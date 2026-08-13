import {
  GODOT_LIMITS,
  type GDScriptCompletionItem,
  type GDScriptCompletionResult,
  type GDScriptDefinitionLocation,
  type GDScriptDefinitionResult,
  type GDScriptHoverResult,
  type GDScriptHoverSection,
  type GDScriptSourceRange,
  type GodotGDScriptDiagnostic,
} from "@siralos/core";
import { sanitizeControlCharacters } from "../diagnostics/diagnostic-normalizer.js";
import { truncateUtf8Bytes } from "../knowledge/api-dump-with-docs.js";
import { mirrorUriToWorkspaceRelative } from "./file-uri.js";

/**
 * Conservative normalization of LSP payloads into the provider-neutral
 * models. Mirror URIs map to workspace-relative paths; out-of-mirror URIs
 * are rejected or represented conservatively; every field is bounded;
 * control characters are sanitized; markup is data (never executed or
 * rendered); and malformed items are skipped safely. LSP line/character
 * positions are 0-based and converted to the 1-based Siralos convention
 * explicitly at this boundary.
 */

export interface LSPNormalizationContext {
  readonly mirrorRootPath: string;
  readonly path: string;
}

function to1BasedRange(range: unknown): GDScriptSourceRange | null {
  if (typeof range !== "object" || range === null) {
    return null;
  }
  const start = to1BasedPosition((range as Record<string, unknown>)["start"]);
  const end = to1BasedPosition((range as Record<string, unknown>)["end"]);
  if (start === null || end === null) {
    return null;
  }
  return { start, end };
}

function to1BasedPosition(position: unknown): { line: number; column: number } | null {
  if (typeof position !== "object" || position === null) {
    return null;
  }
  const line = (position as Record<string, unknown>)["line"];
  const character = (position as Record<string, unknown>)["character"];
  if (typeof line !== "number" || typeof character !== "number") {
    return null;
  }
  if (!Number.isInteger(line) || !Number.isInteger(character) || line < 0 || character < 0) {
    return null;
  }
  return { line: line + 1, column: character + 1 };
}

export interface NormalizedPublishDiagnostics {
  readonly path: string;
  readonly diagnostics: readonly GodotGDScriptDiagnostic[];
  readonly truncated: boolean;
}

/**
 * Normalizes `textDocument/publishDiagnostics`. Out-of-mirror URIs are
 * rejected (returns null); severity 1=error, 2=warning, 3=info, 4=hint
 * (info); unknown severities are preserved as `unknown`; line/column are
 * converted 0-based → 1-based; bounded related information is folded into
 * the message; message size and per-document count are bounded.
 */
export function normalizePublishDiagnostics(
  uri: string,
  rawDiagnostics: unknown,
  context: LSPNormalizationContext,
  limits: { readonly maxDiagnostics: number } = {
    maxDiagnostics: GODOT_LIMITS.lspMaxDiagnosticsPerDocument,
  },
): NormalizedPublishDiagnostics | null {
  const path = mirrorUriToWorkspaceRelative(uri, context.mirrorRootPath);
  if (path === null) {
    return null;
  }
  if (!Array.isArray(rawDiagnostics)) {
    return null;
  }
  const diagnostics: GodotGDScriptDiagnostic[] = [];
  let truncated = false;
  for (const entry of rawDiagnostics) {
    if (diagnostics.length >= limits.maxDiagnostics) {
      truncated = true;
      break;
    }
    if (typeof entry !== "object" || entry === null) {
      continue;
    }
    const record = entry as Record<string, unknown>;
    const range = to1BasedRange(record["range"]);
    const severity = mapSeverity(record["severity"]);
    const code = mapCode(record["code"]);
    const message = boundMessage(record["message"], context);
    if (message.length === 0) {
      continue;
    }
    diagnostics.push({
      source: "godot-lsp",
      severity,
      path,
      line: range?.start.line ?? null,
      column: range?.start.column ?? null,
      code,
      message,
      rawCategory: typeof record["source"] === "string" ? record["source"] : null,
    });
  }
  return { path, diagnostics, truncated };
}

function mapSeverity(value: unknown): GodotGDScriptDiagnostic["severity"] {
  if (value === 1) {
    return "error";
  }
  if (value === 2) {
    return "warning";
  }
  if (value === 3 || value === 4) {
    return "info";
  }
  return "unknown";
}

function mapCode(value: unknown): string | null {
  if (typeof value === "string") {
    return truncateUtf8Bytes(value, GODOT_LIMITS.maxDiagnosticMessageBytes);
  }
  if (typeof value === "number") {
    return String(value);
  }
  return null;
}

function boundMessage(value: unknown, context: LSPNormalizationContext): string {
  if (typeof value !== "string") {
    return "";
  }
  let text = sanitizeControlCharacters(value).trim();
  text = text.split(context.mirrorRootPath).join("<mirror>");
  return truncateUtf8Bytes(text, GODOT_LIMITS.maxDiagnosticMessageBytes);
}

/** One hover section; markup is data, never executed or rendered. */
export function normalizeHover(
  uri: string,
  hover: unknown,
  context: LSPNormalizationContext,
): GDScriptHoverResult | null {
  const path = mirrorUriToWorkspaceRelative(uri, context.mirrorRootPath);
  if (path === null || hover === null || hover === undefined) {
    return null;
  }
  const contents = extractHoverContents(hover, context);
  const range =
    typeof hover === "object" && !Array.isArray(hover)
      ? to1BasedRange((hover as Record<string, unknown>)["range"])
      : null;
  return { path, range, contents };
}

function extractHoverContents(
  hover: unknown,
  context: LSPNormalizationContext,
): readonly GDScriptHoverSection[] {
  if (typeof hover === "object" && hover !== null && !Array.isArray(hover)) {
    const value = (hover as Record<string, unknown>)["contents"];
    return sectionsFromContents(value, context);
  }
  return sectionsFromContents(hover, context);
}

function sectionsFromContents(
  value: unknown,
  context: LSPNormalizationContext,
): readonly GDScriptHoverSection[] {
  const sections: GDScriptHoverSection[] = [];
  let totalBytes = 0;
  const push = (kind: "plaintext" | "markdown", text: string): void => {
    const sanitized = sanitizeControlCharacters(text)
      .split(context.mirrorRootPath)
      .join("<mirror>");
    const remaining = Math.max(GODOT_LIMITS.lspMaxHoverBytes - totalBytes, 0);
    const bounded = truncateUtf8Bytes(sanitized, remaining);
    totalBytes += Buffer.byteLength(bounded, "utf8");
    sections.push({ kind, text: bounded });
  };
  const visit = (entry: unknown): void => {
    if (totalBytes >= GODOT_LIMITS.lspMaxHoverBytes) {
      return;
    }
    if (typeof entry === "string") {
      push("plaintext", entry);
      return;
    }
    if (Array.isArray(entry)) {
      for (const item of entry) {
        visit(item);
      }
      return;
    }
    if (typeof entry === "object" && entry !== null) {
      const record = entry as Record<string, unknown>;
      if (typeof record["kind"] === "string" && typeof record["value"] === "string") {
        push(record["kind"] === "markdown" ? "markdown" : "plaintext", record["value"]);
        return;
      }
      if (typeof record["language"] === "string" && typeof record["value"] === "string") {
        push("plaintext", record["value"]);
      }
    }
  };
  visit(value);
  return sections;
}

export function normalizeCompletion(
  uri: string,
  completion: unknown,
  context: LSPNormalizationContext,
): GDScriptCompletionResult {
  const path = mirrorUriToWorkspaceRelative(uri, context.mirrorRootPath) ?? context.path;
  const rawItems = Array.isArray(completion)
    ? completion
    : typeof completion === "object" && completion !== null
      ? (completion as Record<string, unknown>)["items"]
      : null;
  const items: GDScriptCompletionItem[] = [];
  let truncated = false;
  if (Array.isArray(rawItems)) {
    for (const entry of rawItems) {
      if (items.length >= GODOT_LIMITS.lspMaxCompletionItems) {
        truncated = true;
        break;
      }
      const item = normalizeCompletionItem(entry, context);
      if (item !== null) {
        items.push(item);
      }
    }
  }
  return { path, items, truncated };
}

function normalizeCompletionItem(
  entry: unknown,
  context: LSPNormalizationContext,
): GDScriptCompletionItem | null {
  if (typeof entry !== "object" || entry === null) {
    return null;
  }
  const record = entry as Record<string, unknown>;
  const label =
    typeof record["label"] === "string" ? sanitizeControlCharacters(record["label"]) : null;
  if (label === null || label.length === 0) {
    return null;
  }
  // `additionalTextEdits` and `command` attachments are deliberately
  // dropped: completion never mutates files or executes commands.
  const kind = typeof record["kind"] === "number" ? String(record["kind"]) : null;
  const detail = boundedDetail(record["detail"], context);
  const documentation = boundedDocumentation(record["documentation"], context);
  const insertText =
    typeof record["insertText"] === "string" ? boundedDetail(record["insertText"], context) : null;
  return {
    label: truncateUtf8Bytes(label, GODOT_LIMITS.lspMaxHoverBytes),
    kind,
    detail,
    documentation,
    insertText,
  };
}

function boundedDetail(value: unknown, context: LSPNormalizationContext): string | null {
  if (typeof value !== "string") {
    return null;
  }
  return truncateUtf8Bytes(
    sanitizeControlCharacters(value).split(context.mirrorRootPath).join("<mirror>"),
    GODOT_LIMITS.lspMaxHoverBytes,
  );
}

function boundedDocumentation(value: unknown, context: LSPNormalizationContext): string | null {
  if (typeof value === "string") {
    return truncateUtf8Bytes(
      sanitizeControlCharacters(value).split(context.mirrorRootPath).join("<mirror>"),
      GODOT_LIMITS.lspMaxHoverBytes,
    );
  }
  if (typeof value === "object" && value !== null) {
    const record = value as Record<string, unknown>;
    if (typeof record["value"] === "string") {
      return truncateUtf8Bytes(
        sanitizeControlCharacters(record["value"]).split(context.mirrorRootPath).join("<mirror>"),
        GODOT_LIMITS.lspMaxHoverBytes,
      );
    }
  }
  return null;
}

export function normalizeDefinition(
  uri: string,
  locations: unknown,
  context: LSPNormalizationContext,
): GDScriptDefinitionResult {
  const path = mirrorUriToWorkspaceRelative(uri, context.mirrorRootPath) ?? context.path;
  const rawLocations = Array.isArray(locations)
    ? locations
    : locations === null || locations === undefined
      ? []
      : [locations];
  const result: GDScriptDefinitionLocation[] = [];
  let truncated = false;
  for (const entry of rawLocations) {
    if (result.length >= GODOT_LIMITS.lspMaxDefinitionLocations) {
      truncated = true;
      break;
    }
    const location = normalizeDefinitionLocation(entry, context);
    if (location !== null) {
      result.push(location);
    }
  }
  return { path, locations: result, truncated };
}

function normalizeDefinitionLocation(
  entry: unknown,
  context: LSPNormalizationContext,
): GDScriptDefinitionLocation | null {
  if (typeof entry !== "object" || entry === null) {
    return null;
  }
  const record = entry as Record<string, unknown>;
  // LocationLink: use targetUri + targetRange.
  const uri =
    typeof record["targetUri"] === "string"
      ? record["targetUri"]
      : typeof record["uri"] === "string"
        ? record["uri"]
        : null;
  const rangeValue = record["targetRange"] !== undefined ? record["targetRange"] : record["range"];
  if (uri === null) {
    return null;
  }
  const range = to1BasedRange(rangeValue);
  if (range === null) {
    return null;
  }
  const relative = mirrorUriToWorkspaceRelative(uri, context.mirrorRootPath);
  if (relative !== null) {
    // Only mirror-file definitions map back to workspace-relative paths.
    return { path: relative, range, external: false };
  }
  // Out-of-project and engine-internal URIs are represented conservatively
  // without absolute paths.
  const basename =
    uri
      .split("/")
      .filter((part) => part.length > 0)
      .pop() ?? "external";
  return { path: sanitizeControlCharacters(basename), range, external: true };
}
