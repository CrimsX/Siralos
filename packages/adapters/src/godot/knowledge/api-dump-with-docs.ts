import { createHash } from "node:crypto";
import { GODOT_LIMITS } from "@solaris/core";

/**
 * Normalized, bounded representation of an engine-generated
 * `extension_api.json` (from `--dump-extension-api-with-docs`). Every field
 * is optional: Godot's dump format is not a formal versioned protocol, so
 * parsing is conservative and unknown fields are tolerated without failing
 * the build.
 */

export interface GodotApiDumpParameter {
  readonly name: string;
  readonly type: string;
  readonly defaultValue: string | null;
}

export interface GodotApiDumpMethod {
  readonly name: string;
  readonly returnType: string | null;
  readonly parameters: readonly GodotApiDumpParameter[];
  readonly qualifiers: readonly string[];
  readonly hash: string | null;
  readonly description: string | null;
}

export interface GodotApiDumpProperty {
  readonly name: string;
  readonly type: string | null;
  readonly setter: string | null;
  readonly getter: string | null;
  readonly description: string | null;
}

export interface GodotApiDumpSignal {
  readonly name: string;
  readonly parameters: readonly GodotApiDumpParameter[];
  readonly description: string | null;
}

export interface GodotApiDumpConstant {
  readonly name: string;
  /** String representation of the value when representable. */
  readonly value: string | null;
  readonly description: string | null;
}

export interface GodotApiDumpEnum {
  readonly name: string;
  readonly values: readonly { readonly name: string; readonly value: string }[];
  readonly description: string | null;
}

export interface GodotApiDumpClass {
  readonly name: string;
  readonly baseClass: string | null;
  readonly apiType: string | null;
  readonly briefDescription: string | null;
  readonly description: string | null;
  readonly methods: readonly GodotApiDumpMethod[];
  readonly properties: readonly GodotApiDumpProperty[];
  readonly signals: readonly GodotApiDumpSignal[];
  readonly constants: readonly GodotApiDumpConstant[];
  readonly enums: readonly GodotApiDumpEnum[];
}

export interface GodotApiDumpBuiltinClass {
  readonly name: string;
  readonly description: string | null;
  readonly methods: readonly GodotApiDumpMethod[];
  readonly operators: readonly { readonly name: string }[];
  readonly constants: readonly GodotApiDumpConstant[];
  readonly enums: readonly GodotApiDumpEnum[];
}

export interface GodotApiDumpDocument {
  readonly versionFullName: string | null;
  readonly hash: string | null;
  readonly classes: readonly GodotApiDumpClass[];
  readonly builtinClasses: readonly GodotApiDumpBuiltinClass[];
  readonly globalConstants: readonly GodotApiDumpConstant[];
  readonly globalEnums: readonly GodotApiDumpEnum[];
  readonly utilityFunctions: readonly GodotApiDumpMethod[];
  readonly rawBytes: number;
  readonly sha256: string;
}

export type GodotApiDumpParseResult =
  | { readonly ok: true; readonly document: GodotApiDumpDocument }
  | { readonly ok: false; readonly message: string };

const MAX_DESCRIPTION_BYTES = GODOT_LIMITS.maxApiDescriptionBytes;

/**
 * Parses the exact bytes of a with-docs `extension_api.json` into a bounded
 * normalized document. The raw dump is never persisted into the workspace,
 * never becomes an application/provider event, and its SHA-256 is computed
 * over the exact raw bytes.
 */
export function parseGodotApiDumpWithDocs(content: Buffer): GodotApiDumpParseResult {
  let parsed: unknown;
  try {
    parsed = JSON.parse(content.toString("utf8"));
  } catch {
    return { ok: false, message: "The API documentation dump is not valid JSON." };
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return { ok: false, message: "The API documentation dump is not a JSON object." };
  }
  const root = parsed as Record<string, unknown>;
  const header = asRecord(root["header"]);
  const versionFullName =
    header !== null && typeof header["version_full_name"] === "string"
      ? header["version_full_name"]
      : null;
  const hash = header !== null && typeof header["hash"] === "string" ? header["hash"] : null;
  const classes = (asArray(root["classes"]) ?? []).map(parseClass);
  const builtinClasses = (asArray(root["builtin_classes"]) ?? []).map(parseBuiltinClass);
  const globalConstants = (asArray(root["global_constants"]) ?? []).map((entry) =>
    parseConstant(asRecord(entry)),
  );
  const globalEnums = (asArray(root["global_enums"]) ?? []).map((entry) =>
    parseEnum(asRecord(entry)),
  );
  const utilityFunctions = (asArray(root["utility_functions"]) ?? []).map((entry) =>
    parseMethod(asRecord(entry)),
  );
  const sha256 = createHash("sha256").update(content).digest("hex");
  return {
    ok: true,
    document: {
      versionFullName,
      hash,
      classes,
      builtinClasses,
      globalConstants,
      globalEnums,
      utilityFunctions,
      rawBytes: content.length,
      sha256,
    },
  };
}

function parseClass(entry: unknown): GodotApiDumpClass {
  const record = asRecord(entry) ?? {};
  return {
    name: boundedString(record["name"]) ?? "unnamed",
    baseClass: nullableString(record["base_class"]),
    apiType: nullableString(record["api_type"]),
    briefDescription: boundedDescription(record["brief_description"]),
    description: boundedDescription(record["description"]),
    methods: (asArray(record["methods"]) ?? []).map((method) => parseMethod(asRecord(method))),
    properties: (asArray(record["properties"]) ?? []).map((property) =>
      parseProperty(asRecord(property)),
    ),
    signals: (asArray(record["signals"]) ?? []).map((signal) => parseSignal(asRecord(signal))),
    constants: (asArray(record["constants"]) ?? []).map((constant) =>
      parseConstant(asRecord(constant)),
    ),
    enums: (asArray(record["enums"]) ?? []).map((entry) => parseEnum(asRecord(entry))),
  };
}

function parseBuiltinClass(entry: unknown): GodotApiDumpBuiltinClass {
  const record = asRecord(entry) ?? {};
  return {
    name: boundedString(record["name"]) ?? "unnamed",
    description: boundedDescription(record["description"]),
    methods: (asArray(record["methods"]) ?? []).map((method) => parseMethod(asRecord(method))),
    operators: (asArray(record["operators"]) ?? [])
      .map((operator) => asRecord(operator))
      .filter((operator) => operator !== null)
      .map((operator) => ({ name: boundedString(operator?.["name"]) ?? "?" })),
    constants: (asArray(record["constants"]) ?? []).map((constant) =>
      parseConstant(asRecord(constant)),
    ),
    enums: (asArray(record["enums"]) ?? []).map((entry) => parseEnum(asRecord(entry))),
  };
}

function parseMethod(record: Record<string, unknown> | null): GodotApiDumpMethod {
  const source = record ?? {};
  const qualifiers: string[] = [];
  if (source["is_static"] === true) {
    qualifiers.push("static");
  }
  if (source["is_vararg"] === true) {
    qualifiers.push("vararg");
  }
  if (source["is_const"] === true) {
    qualifiers.push("const");
  }
  if (source["is_virtual"] === true) {
    qualifiers.push("virtual");
  }
  return {
    name: boundedString(source["name"]) ?? "unnamed",
    returnType: nullableString(source["return_type"]),
    parameters: (asArray(source["arguments"]) ?? []).map((argument) =>
      parseParameter(asRecord(argument)),
    ),
    qualifiers,
    hash: nullableHash(source["hash"]),
    description: boundedDescription(source["description"]),
  };
}

function parseParameter(record: Record<string, unknown> | null): GodotApiDumpParameter {
  const source = record ?? {};
  return {
    name: boundedString(source["name"]) ?? "arg",
    type: boundedString(source["type"]) ?? "Variant",
    defaultValue: representableValue(source["default_value"]),
  };
}

function parseProperty(record: Record<string, unknown> | null): GodotApiDumpProperty {
  const source = record ?? {};
  return {
    name: boundedString(source["name"]) ?? "unnamed",
    type: nullableString(source["type"]),
    setter: nullableString(source["setter"]),
    getter: nullableString(source["getter"]),
    description: boundedDescription(source["description"]),
  };
}

function parseSignal(record: Record<string, unknown> | null): GodotApiDumpSignal {
  const source = record ?? {};
  return {
    name: boundedString(source["name"]) ?? "unnamed",
    parameters: (asArray(source["arguments"]) ?? []).map((argument) =>
      parseParameter(asRecord(argument)),
    ),
    description: boundedDescription(source["description"]),
  };
}

function parseConstant(record: Record<string, unknown> | null): GodotApiDumpConstant {
  const source = record ?? {};
  return {
    name: boundedString(source["name"]) ?? "unnamed",
    value: representableValue(source["value"]),
    description: boundedDescription(source["description"]),
  };
}

function parseEnum(record: Record<string, unknown> | null): GodotApiDumpEnum {
  const source = record ?? {};
  const values = (asArray(source["values"]) ?? [])
    .map((entry) => asRecord(entry))
    .filter((entry) => entry !== null)
    .map((entry) => ({
      name: boundedString(entry?.["name"]) ?? "unnamed",
      value: representableValue(entry?.["value"]) ?? "?",
    }));
  return {
    name: boundedString(source["name"]) ?? "unnamed",
    values,
    description: boundedDescription(source["description"]),
  };
}

function representableValue(value: unknown): string | null {
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    const text = typeof value === "string" ? `"${value}"` : String(value);
    return truncateUtf8Bytes(text, MAX_DESCRIPTION_BYTES);
  }
  return null;
}

function boundedDescription(value: unknown): string | null {
  const text = boundedString(value);
  return text === null ? null : truncateUtf8Bytes(text, MAX_DESCRIPTION_BYTES);
}

function boundedString(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }
  return truncateUtf8Bytes(value, MAX_DESCRIPTION_BYTES);
}

function nullableString(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }
  return truncateUtf8Bytes(value, MAX_DESCRIPTION_BYTES);
}

/** Method hashes are numbers in the dump; they are retained as text. */
function nullableHash(value: unknown): string | null {
  if (typeof value === "number") {
    return String(value);
  }
  return nullableString(value);
}

/** Truncates UTF-8 text to an exact byte bound without splitting a code point. */
export function truncateUtf8Bytes(text: string, maxBytes: number): string {
  if (Buffer.byteLength(text, "utf8") <= maxBytes) {
    return text;
  }
  let result = "";
  let bytes = 0;
  for (const character of text) {
    const size = Buffer.byteLength(character, "utf8");
    if (bytes + size > maxBytes) {
      break;
    }
    result += character;
    bytes += size;
  }
  return result;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function asArray(value: unknown): unknown[] | null {
  return Array.isArray(value) ? value : null;
}
