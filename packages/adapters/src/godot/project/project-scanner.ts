import type { GodotAutoloadSummary, SafeDiagnostic } from "@solaris/core";

/** Bounded structural record of one project.godot property. */
export interface ScannedProjectProperty {
  readonly section: string;
  readonly key: string;
  /** Raw bounded value text exactly as scanned. */
  readonly rawValue: string;
  readonly lineNumber: number;
}

export type ScannedValueKind =
  "string" | "integer" | "boolean" | "packed-string-array" | "uid" | "raw";

export interface ScannedValue {
  readonly kind: ScannedValueKind;
  /** Interpreted value for supported kinds; null for raw. */
  readonly value: string | number | boolean | readonly string[] | null;
  readonly raw: string;
}

export interface GodotProjectScanResult {
  readonly properties: readonly ScannedProjectProperty[];
  readonly configVersion: number | null;
  readonly name: string | null;
  readonly applicationVersion: string | null;
  readonly declaredFeatures: readonly string[];
  readonly mainScene: string | null;
  readonly renderingMethods: readonly string[];
  readonly dotnetAssemblyName: string | null;
  readonly autoloads: readonly GodotAutoloadSummary[];
  readonly enabledPlugins: readonly string[];
  readonly warnings: readonly SafeDiagnostic[];
  /** True when a bounded scan limit was reached. */
  readonly truncated: boolean;
}

const MAX_SECTIONS = 128;
const MAX_PROPERTIES_PER_SECTION = 4096;
const MAX_LINE_LENGTH = 64 * 1024;
const MAX_WARNINGS = 50;

/**
 * Conservative, purpose-built read-only scanner for untrusted
 * `project.godot` content. Only supported value forms are interpreted:
 * integer literals, booleans, quoted strings (with escapes), and
 * `PackedStringArray(...)`. Unsupported forms are preserved as raw values
 * and reported as unknown; nothing is evaluated, executed, or resolved.
 * Results are static and non-authoritative.
 */
export function scanProjectFile(content: string): GodotProjectScanResult {
  const lines = content.split(/\r?\n/);
  const properties: ScannedProjectProperty[] = [];
  const warnings: SafeDiagnostic[] = [];
  const addWarning = (message: string): void => {
    if (warnings.length < MAX_WARNINGS) {
      warnings.push({ severity: "warning", message });
    }
  };
  let section = "";
  let sectionCount = 0;
  let propertyCount = 0;
  let truncated = false;
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index] ?? "";
    if (line.length > MAX_LINE_LENGTH) {
      truncated = true;
      continue;
    }
    const trimmed = line.trim();
    if (trimmed.length === 0 || isCommentLine(trimmed)) {
      continue;
    }
    if (trimmed.startsWith("[") && trimmed.includes("]")) {
      if (sectionCount >= MAX_SECTIONS) {
        truncated = true;
        continue;
      }
      sectionCount += 1;
      propertyCount = 0;
      section = trimmed.slice(1, trimmed.indexOf("]")).trim();
      continue;
    }
    const equalsIndex = trimmed.indexOf("=");
    if (equalsIndex < 0) {
      addWarning(`Unrecognized project setting without a value at line ${index + 1}.`);
      continue;
    }
    if (propertyCount >= MAX_PROPERTIES_PER_SECTION) {
      truncated = true;
      continue;
    }
    propertyCount += 1;
    const key = unquoteKey(trimmed.slice(0, equalsIndex).trim());
    const rawValue = trimmed.slice(equalsIndex + 1).trim();
    properties.push({ section, key, rawValue, lineNumber: index + 1 });
  }
  const configVersion = readInteger(properties, "", "config_version", warnings);
  const name = readString(properties, "application", "config/name", warnings);
  const applicationVersion = readString(properties, "application", "config/version", warnings);
  const declaredFeatures = readStringArray(properties, "application", "config/features");
  const mainScene = readString(properties, "application", "run/main_scene", warnings);
  const renderingMethods = [
    readString(properties, "rendering", "renderer/rendering_method", warnings),
    readString(properties, "rendering", "renderer/rendering_method.mobile", warnings),
  ].filter((value): value is string => value !== null);
  const dotnetAssemblyName = readString(properties, "dotnet", "project/assembly_name", warnings);
  const autoloads = readAutoloads(properties);
  const enabledPlugins = readStringArray(properties, "editor_plugins", "enabled");
  const distinct = [...new Set(renderingMethods)];
  return {
    properties,
    configVersion,
    name,
    applicationVersion,
    declaredFeatures,
    mainScene,
    renderingMethods: distinct,
    dotnetAssemblyName,
    autoloads,
    enabledPlugins,
    warnings,
    truncated,
  };
}

function isCommentLine(trimmed: string): boolean {
  return trimmed.startsWith(";") || trimmed.startsWith("#");
}

function unquoteKey(key: string): string {
  if (key.length >= 2 && key.startsWith('"') && key.endsWith('"')) {
    return key.slice(1, -1);
  }
  return key;
}

function findProperty(
  properties: readonly ScannedProjectProperty[],
  section: string,
  key: string,
): ScannedProjectProperty | undefined {
  return properties.find((property) => property.section === section && property.key === key);
}

function interpretValue(raw: string): ScannedValue {
  if (raw.startsWith('"')) {
    const parsed = parseQuotedString(raw);
    if (parsed.ok) {
      return { kind: "string", value: parsed.value, raw };
    }
    return { kind: "raw", value: null, raw };
  }
  if (raw.startsWith("PackedStringArray(") && raw.endsWith(")")) {
    const inner = raw.slice("PackedStringArray(".length, -1).trim();
    const items = parseCommaSeparatedStrings(inner);
    if (items !== null) {
      return { kind: "packed-string-array", value: items, raw };
    }
    return { kind: "raw", value: null, raw };
  }
  if (/^[+-]?\d+$/.test(raw)) {
    return { kind: "integer", value: Number(raw), raw };
  }
  if (raw === "true") {
    return { kind: "boolean", value: true, raw };
  }
  if (raw === "false") {
    return { kind: "boolean", value: false, raw };
  }
  if (/^uid:\/\/[0-9a-z]+$/i.test(raw)) {
    return { kind: "uid", value: raw, raw };
  }
  return { kind: "raw", value: null, raw };
}

export function parseQuotedString(
  raw: string,
  options: { readonly allowTrailing?: boolean } = {},
):
  | { readonly ok: true; readonly value: string; readonly consumed: number }
  | { readonly ok: false } {
  if (!raw.startsWith('"')) {
    return { ok: false };
  }
  let value = "";
  let index = 1;
  let closed = false;
  while (index < raw.length) {
    const character = raw[index] as string;
    if (character === '"') {
      closed = true;
      index += 1;
      break;
    }
    if (character === "\\") {
      const next = raw[index + 1];
      if (next === "n") {
        value += "\n";
      } else if (next === "t") {
        value += "\t";
      } else if (next === "r") {
        value += "\r";
      } else if (next === '"' || next === "\\" || next === "'") {
        value += next;
      } else if (next === undefined) {
        break;
      } else {
        value += `\\${next}`;
      }
      index += 2;
      continue;
    }
    value += character;
    index += 1;
  }
  if (!closed) {
    return { ok: false };
  }
  if (options.allowTrailing !== true) {
    const remainder = raw.slice(index).trim();
    if (remainder.length > 0) {
      return { ok: false };
    }
  }
  return { ok: true, value, consumed: index };
}

function parseCommaSeparatedStrings(text: string): readonly string[] | null {
  if (text.length === 0) {
    return [];
  }
  const items: string[] = [];
  let index = 0;
  while (index < text.length) {
    while (index < text.length && (text[index] === "," || /\s/.test(text[index] as string))) {
      index += 1;
    }
    if (index >= text.length) {
      break;
    }
    const segment = text.slice(index);
    const parsed = parseQuotedString(segment, { allowTrailing: true });
    if (!parsed.ok) {
      return null;
    }
    items.push(parsed.value);
    index += parsed.consumed;
  }
  return items;
}

function readInteger(
  properties: readonly ScannedProjectProperty[],
  section: string,
  key: string,
  warnings: SafeDiagnostic[],
): number | null {
  const property = findProperty(properties, section, key);
  if (property === undefined) {
    return null;
  }
  const value = interpretValue(property.rawValue);
  if (value.kind !== "integer") {
    warnUnknown(warnings, property);
    return null;
  }
  return value.value as number;
}

function readString(
  properties: readonly ScannedProjectProperty[],
  section: string,
  key: string,
  warnings: SafeDiagnostic[],
): string | null {
  const property = findProperty(properties, section, key);
  if (property === undefined) {
    return null;
  }
  const value = interpretValue(property.rawValue);
  if (value.kind !== "string") {
    warnUnknown(warnings, property);
    return null;
  }
  return value.value as string;
}

function readStringArray(
  properties: readonly ScannedProjectProperty[],
  section: string,
  key: string,
): readonly string[] {
  const property = findProperty(properties, section, key);
  if (property === undefined) {
    return [];
  }
  const value = interpretValue(property.rawValue);
  if (value.kind !== "packed-string-array") {
    return [];
  }
  return value.value as readonly string[];
}

function readAutoloads(
  properties: readonly ScannedProjectProperty[],
): readonly GodotAutoloadSummary[] {
  const autoloads: GodotAutoloadSummary[] = [];
  for (const property of properties) {
    if (property.section !== "autoload") {
      continue;
    }
    const value = interpretValue(property.rawValue);
    if (value.kind !== "string") {
      continue;
    }
    const target = value.value as string;
    autoloads.push({
      name: property.key,
      target,
      isSingleton: target.startsWith("*"),
    });
  }
  return autoloads;
}

function warnUnknown(warnings: SafeDiagnostic[], property: ScannedProjectProperty): void {
  if (warnings.length >= MAX_WARNINGS) {
    return;
  }
  warnings.push({
    severity: "warning",
    message: `The value of ${property.section}.${property.key} (line ${property.lineNumber}) uses an unsupported form and was preserved as unknown.`,
  });
}
